//! Pure conversion: Workstate `Finding`s -> ScriptVault sidecar records.
//!
//! This module is the bridge's whole reason to exist, kept pure (data in,
//! data out, no I/O) per the suite's ingest/normalize split. The rules are
//! ported from the retired Python bridge so ScriptVault sees the exact same
//! metadata shape it always has:
//!
//! * tags: `risk:<risk>` and `owner:<owner>` (bridge-managed prefixes)
//! * desc: Bulwark's description, whitespace-flattened, capped at 200 chars,
//!   with a trailing `[RISK: …]` badge for medium/high/critical subjects
//!
//! Like Workstate's own adapters, conversion is INFALLIBLE: bad records are
//! skipped (and reported), never an error — one malformed finding must not
//! sink the feed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use workstate::model::normalized::Finding;

/// Suffix ScriptVault uses for sidecar files; paths already pointing at a
/// sidecar are never themselves annotated.
pub const SIDECAR_SUFFIX: &str = ".scriptvault.yaml";
/// Bridge-managed tag prefixes (ScriptVault users own every other tag).
pub const RISK_TAG_PREFIX: &str = "risk:";
pub const OWNER_TAG_PREFIX: &str = "owner:";
/// Description badge cutoff: only these risks are loud enough to badge.
const BADGE_RISK_LEVELS: [&str; 3] = ["medium", "high", "critical"];
/// Sidecar descriptions stay scannable in ScriptVault's list view.
const MAX_DESC_LEN: usize = 200;

/// One sidecar's worth of metadata for one script, in ScriptVault's own
/// sidecar field names (`tags`, `desc` — a subset of its `ScriptMetadata`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidecarRecord {
    /// Absolute path of the script the sidecar annotates.
    pub path: String,
    /// Bridge-managed tags (`risk:…`, `owner:…`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Short description with optional risk badge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
}

/// Why a finding contributed no sidecar record. Reported, never fatal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skip {
    /// The finding's subject id (what Bulwark scanned).
    pub subject: String,
    /// Human-readable reason the record was dropped.
    pub reason: &'static str,
}

/// The outcome of a conversion: the records to publish plus everything that
/// was dropped and why (so the CLI can report skips honestly).
#[derive(Debug, Default)]
pub struct Conversion {
    pub sidecars: Vec<SidecarRecord>,
    pub skipped: Vec<Skip>,
}

/// Convert Bulwark findings into sidecar records, one per distinct script
/// path, sorted by path (deterministic output for diffable feeds).
///
/// Several findings can share one subject (several rules firing on the same
/// script), so findings are grouped by path first. Within a group the
/// HIGHEST-severity finding is the representative: its risk drives the tag
/// and badge. Owner and description fall back across the group so partial
/// records still contribute what they have.
pub fn convert(findings: &[Finding]) -> Conversion {
    let mut conversion = Conversion::default();

    // Group by script path. BTreeMap keeps the output path-sorted for free.
    let mut by_path: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
    for finding in findings {
        let Some(path) = finding_path(finding) else {
            conversion.skipped.push(Skip {
                subject: finding.id.0.clone(),
                reason: "no usable script path",
            });
            continue;
        };
        if path.ends_with(SIDECAR_SUFFIX) {
            conversion.skipped.push(Skip {
                subject: finding.id.0.clone(),
                reason: "path is itself a sidecar",
            });
            continue;
        }
        by_path.entry(path).or_default().push(finding);
    }

    for (path, group) in by_path {
        // Highest severity first; ties keep feed order (stable sort).
        let mut group = group;
        group.sort_by_key(|f| std::cmp::Reverse(f.severity));
        let representative = group[0];

        let mut tags = Vec::new();
        if let Some(risk) = risk_of(representative) {
            tags.push(format!("{RISK_TAG_PREFIX}{risk}"));
        }
        if let Some(owner) = group.iter().find_map(|f| owner_of(f)) {
            tags.push(format!("{OWNER_TAG_PREFIX}{owner}"));
        }

        let desc = build_desc(&group, representative);

        if tags.is_empty() && desc.is_none() {
            conversion.skipped.push(Skip {
                subject: representative.id.0.clone(),
                reason: "finding carries no risk, owner, or description",
            });
            continue;
        }

        conversion.sidecars.push(SidecarRecord {
            path: path.to_string(),
            tags,
            desc,
        });
    }

    conversion
}

/// The script path a finding refers to: Bulwark's `path` pass-through field,
/// kept by Workstate in `Finding.rest`. `None` (skip) when absent, blank, or
/// not a string — the bridge cannot place a sidecar without a real path.
fn finding_path(finding: &Finding) -> Option<&str> {
    rest_str(finding, "path")
}

/// The risk label for the tag and badge: Bulwark's `risk` pass-through field,
/// falling back to the feed's raw severity string. The canonical `Severity`
/// buckets (`Unrated`/`Unknown`) are deliberately NOT used as labels — "no
/// risk signal" must not become a `risk:unrated` tag.
fn risk_of(finding: &Finding) -> Option<&str> {
    rest_str(finding, "risk").or_else(|| non_blank(finding.raw_severity.as_deref()))
}

/// The owning user/team, from Bulwark's `owner` pass-through field.
fn owner_of(finding: &Finding) -> Option<&str> {
    rest_str(finding, "owner")
}

/// A non-blank string field out of a finding's pass-through `rest` map.
fn rest_str<'a>(finding: &'a Finding, key: &str) -> Option<&'a str> {
    non_blank(finding.rest.get(key).and_then(Value::as_str))
}

fn non_blank(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

/// Build the sidecar description: the representative's description (first
/// non-blank in the group as fallback), flattened and capped, plus the risk
/// badge for medium/high/critical. A badge with no description stands alone.
fn build_desc(group: &[&Finding], representative: &Finding) -> Option<String> {
    let raw = group
        .iter()
        .find_map(|f| non_blank(f.description.as_deref()));

    let mut desc = raw.map(flatten_desc).filter(|d| !d.is_empty());

    if let Some(badge) = desc_badge(risk_of(representative)) {
        desc = Some(match desc {
            Some(d) => format!("{d}  {badge}"),
            None => badge,
        });
    }
    desc
}

/// Flatten a Bulwark description for one-line display: collapse all
/// whitespace runs, strip leading separator junk (`─-—=_`), cap at
/// `MAX_DESC_LEN` characters (char-safe), and trim the cut edge.
fn flatten_desc(raw: &str) -> String {
    let flat = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = flat.trim_start_matches(['─', '-', '—', '=', '_', ' ']);
    trimmed
        .chars()
        .take(MAX_DESC_LEN)
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// `[RISK: HIGH]`-style badge for risks loud enough to warrant one.
fn desc_badge(risk: Option<&str>) -> Option<String> {
    let risk = risk?.trim();
    if BADGE_RISK_LEVELS
        .iter()
        .any(|level| risk.eq_ignore_ascii_case(level))
    {
        Some(format!("[RISK: {}]", risk.to_uppercase()))
    } else {
        None
    }
}

/// True for tags the bridge owns (and would overwrite on re-run); ScriptVault
/// keeps every other tag untouched when it merges this feed.
pub fn is_bridge_tag(tag: &str) -> bool {
    tag.starts_with(RISK_TAG_PREFIX) || tag.starts_with(OWNER_TAG_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use workstate::model::normalized::{FindingId, Severity};

    /// A finding with the pass-through fields Bulwark's workstate-feed carries.
    fn finding(
        path: Option<&str>,
        risk: Option<&str>,
        owner: Option<&str>,
        desc: Option<&str>,
        severity: Severity,
    ) -> Finding {
        let mut rest = BTreeMap::new();
        if let Some(p) = path {
            rest.insert("path".to_string(), Value::String(p.to_string()));
        }
        if let Some(r) = risk {
            rest.insert("risk".to_string(), Value::String(r.to_string()));
        }
        if let Some(o) = owner {
            rest.insert("owner".to_string(), Value::String(o.to_string()));
        }
        Finding {
            id: FindingId(path.unwrap_or("subject").to_string()),
            name: None,
            rule_id: None,
            description: desc.map(str::to_string),
            severity,
            raw_severity: None,
            category: None,
            location: "unknown".to_string(),
            rest,
        }
    }

    #[test]
    fn builds_risk_and_owner_tags() {
        let f = finding(
            Some("/x/a.sh"),
            Some("high"),
            Some("tom"),
            None,
            Severity::High,
        );
        let out = convert(&[f]);
        assert_eq!(out.sidecars.len(), 1);
        assert_eq!(out.sidecars[0].tags, vec!["risk:high", "owner:tom"]);
    }

    #[test]
    fn badges_medium_high_critical_only() {
        assert_eq!(desc_badge(Some("medium")), Some("[RISK: MEDIUM]".into()));
        assert_eq!(desc_badge(Some("HIGH")), Some("[RISK: HIGH]".into()));
        assert_eq!(
            desc_badge(Some("critical")),
            Some("[RISK: CRITICAL]".into())
        );
        assert_eq!(desc_badge(Some("low")), None);
        assert_eq!(desc_badge(Some("info")), None);
        assert_eq!(desc_badge(None), None);
    }

    #[test]
    fn desc_gets_flattened_capped_and_badged() {
        let long = format!("──  weird   header\n\n{}", "x".repeat(300));
        let f = finding(
            Some("/x/a.sh"),
            Some("high"),
            None,
            Some(&long),
            Severity::High,
        );
        let out = convert(&[f]);
        let desc = out.sidecars[0].desc.as_deref().unwrap();
        assert!(desc.starts_with("weird header x"), "got: {desc}");
        assert!(desc.ends_with("  [RISK: HIGH]"));
        // 200 chars of body + the appended badge.
        let body = desc.strip_suffix("  [RISK: HIGH]").unwrap();
        assert_eq!(body.chars().count(), MAX_DESC_LEN);
    }

    #[test]
    fn badge_stands_alone_without_description() {
        let f = finding(
            Some("/x/a.sh"),
            Some("critical"),
            None,
            None,
            Severity::Critical,
        );
        let out = convert(&[f]);
        assert_eq!(out.sidecars[0].desc.as_deref(), Some("[RISK: CRITICAL]"));
    }

    #[test]
    fn skips_findings_without_a_path() {
        let f = finding(None, Some("high"), None, None, Severity::High);
        let out = convert(&[f]);
        assert!(out.sidecars.is_empty());
        assert_eq!(out.skipped.len(), 1);
        assert_eq!(out.skipped[0].reason, "no usable script path");
    }

    #[test]
    fn skips_sidecar_paths() {
        let f = finding(
            Some("/x/a.sh.scriptvault.yaml"),
            Some("low"),
            None,
            None,
            Severity::Low,
        );
        let out = convert(&[f]);
        assert!(out.sidecars.is_empty());
        assert_eq!(out.skipped[0].reason, "path is itself a sidecar");
    }

    #[test]
    fn skips_findings_with_no_metadata_at_all() {
        let f = finding(Some("/x/a.sh"), None, None, None, Severity::Unrated);
        let out = convert(&[f]);
        assert!(out.sidecars.is_empty());
        assert_eq!(
            out.skipped[0].reason,
            "finding carries no risk, owner, or description"
        );
    }

    #[test]
    fn risk_falls_back_to_raw_severity() {
        let mut f = finding(Some("/x/a.sh"), None, None, None, Severity::Medium);
        f.raw_severity = Some("medium".to_string());
        let out = convert(&[f]);
        assert_eq!(out.sidecars[0].tags, vec!["risk:medium"]);
        assert_eq!(out.sidecars[0].desc.as_deref(), Some("[RISK: MEDIUM]"));
    }

    #[test]
    fn groups_multiple_findings_by_path_taking_highest_severity() {
        let low = finding(
            Some("/x/a.sh"),
            Some("low"),
            None,
            Some("email found"),
            Severity::Low,
        );
        let crit = finding(
            Some("/x/a.sh"),
            Some("critical"),
            Some("tom"),
            None,
            Severity::Critical,
        );
        let out = convert(&[low, crit]);
        assert_eq!(out.sidecars.len(), 1);
        let rec = &out.sidecars[0];
        // Tag and badge from the critical finding; owner and desc from
        // whichever group member had them.
        assert_eq!(rec.tags, vec!["risk:critical", "owner:tom"]);
        assert_eq!(rec.desc.as_deref(), Some("email found  [RISK: CRITICAL]"));
    }

    #[test]
    fn output_is_sorted_by_path() {
        let b = finding(Some("/x/b.sh"), Some("low"), None, None, Severity::Low);
        let a = finding(Some("/x/a.sh"), Some("low"), None, None, Severity::Low);
        let out = convert(&[b, a]);
        let paths: Vec<_> = out.sidecars.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(paths, vec!["/x/a.sh", "/x/b.sh"]);
    }

    #[test]
    fn non_string_rest_values_are_ignored_gracefully() {
        let mut f = finding(Some("/x/a.sh"), None, None, Some("desc"), Severity::Low);
        f.rest.insert(
            "risk".to_string(),
            Value::Number(serde_json::Number::from(7)),
        );
        f.rest.insert("owner".to_string(), Value::Bool(true));
        let out = convert(&[f]);
        // Malformed risk/owner contribute nothing; the description survives.
        assert!(out.sidecars[0].tags.is_empty());
        assert_eq!(out.sidecars[0].desc.as_deref(), Some("desc"));
    }

    #[test]
    fn bridge_tag_predicate_matches_only_managed_prefixes() {
        assert!(is_bridge_tag("risk:high"));
        assert!(is_bridge_tag("owner:tom"));
        assert!(!is_bridge_tag("favorite"));
        assert!(!is_bridge_tag("riskaverse"));
    }
}
