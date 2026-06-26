//! The verdict model and how Pulse builds it from live suite data.
//!
//! This is where the four readers in [`crate::sources`] become one of the three
//! default-screen states. The model types (`State`, `Verdict`, `Cause`,
//! `SourceMark`) are consumed by the renderer in `main.rs`; everything the
//! screen draws is resolved here first, so rendering stays pure.
//!
//! Precedence, highest first (see PULSE_DESIGN.md "Opening States"):
//!   1. NeedsAttention — there are critical/high findings or a failed job. A
//!      real problem outranks uncertainty: a leaked key must not be hidden just
//!      because one feed is also missing.
//!   2. Incomplete — no real findings, but the suite view can't be trusted:
//!      the snapshot is absent, a tracked source is missing, or a section is on
//!      an unsupported version.
//!   3. Healthy — snapshot current, sources current, nothing needs attention.

use crate::sources::{
    self, Attention, BinaryCheck, BulwarkView, DataDir, Freshness, JobOutcome, RexopsView,
    SnapshotFreshness,
};

// Re-export so the rest of the crate (renderer, interactive views) can name the
// severity type via `verdict::`, the module that owns the verdict vocabulary.
pub use crate::sources::Severity;

/// The three default-screen verdict states.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Healthy,
    NeedsAttention,
    Incomplete,
}

/// One source's freshness as the confidence line renders it. Mirrors
/// `sources::Freshness` but is the renderer's vocabulary; kept separate so the
/// renderer doesn't depend on the sources module.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Current,
    Stale,
    Missing,
}

impl From<Freshness> for Source {
    fn from(f: Freshness) -> Self {
        match f {
            Freshness::Current => Source::Current,
            Freshness::Stale => Source::Stale,
            Freshness::Unavailable => Source::Missing,
        }
    }
}

/// A named source and its freshness (e.g. `workstate` → Current).
pub struct SourceMark {
    pub name: String,
    pub freshness: Source,
}

/// One cause row: what is affected, why it matters, which source reported it.
pub struct Cause {
    pub what: String,
    pub why: String,
    pub source: String,
}

/// Everything the default screen needs to render, already resolved.
pub struct Verdict {
    pub state: State,
    /// Relative age of the underlying data, already formatted (e.g. "2m ago").
    pub age: String,
    pub critical: usize,
    pub high: usize,
    pub confidence_reduced: bool,
    pub unavailable: usize,
    pub stale: usize,
    pub causes: Vec<Cause>,
    pub sources: Vec<SourceMark>,
}

/// All raw suite readings, gathered once. The interactive app holds this so the
/// detail views (Attention, Feeds) can show the *full* lists without re-reading
/// the filesystem on every keypress, while the default screen shows the verdict
/// derived from the same data — one read, one consistent picture.
pub struct Readings {
    pub freshness: SnapshotFreshness,
    pub rexops: Option<RexopsView>,
    pub bulwark: BulwarkView,
    pub jobs: Vec<Job>,
    pub binaries: Vec<BinaryCheck>,
    pub now: Option<i64>,
}

impl Readings {
    /// Read every suite contract under `dir` once. Never fails: each reader is
    /// individually fault-tolerant.
    pub fn load(dir: &DataDir) -> Self {
        // One read of the single source of truth (the Workstate snapshot); every
        // view below is DERIVED from it. Pulse no longer reads any raw producer
        // feed — if it needs a datum, the datum lives in the snapshot.
        let snap = sources::load(dir);
        Readings {
            freshness: sources::freshness(snap.as_ref()),
            rexops: sources::suite_view(snap.as_ref()),
            bulwark: sources::bulwark(snap.as_ref()),
            jobs: sources::jobs(snap.as_ref()),
            binaries: sources::read_binaries(),
            now: unix_now(),
        }
    }

    /// Empty readings — no snapshot, no feeds. Backs a one-shot render driven by a
    /// pre-built verdict (e.g. a `--state` demo), where the default screen shows
    /// the verdict directly and there are no drill-down lists to populate.
    pub fn empty() -> Self {
        Readings {
            freshness: SnapshotFreshness {
                built_at: None,
                sections: Vec::new(),
            },
            rexops: None,
            bulwark: BulwarkView {
                attention: Vec::new(),
                present: false,
            },
            jobs: Vec::new(),
            binaries: Vec::new(),
            now: None,
        }
    }

    /// Every attention item across producers, most-severe first — the full list
    /// the Attention view shows (the verdict only surfaces the top few). Mirrors
    /// the merge rules in `compose`.
    pub fn all_attention(&self) -> Vec<Attention> {
        let mut items: Vec<Attention> = Vec::new();
        if let Some(rx) = &self.rexops {
            items.extend(rx.attention.iter().map(clone_attention));
        }
        if items.is_empty() {
            items.extend(self.bulwark.attention.iter().map(clone_attention));
        }
        for j in &self.jobs {
            if j.outcome == JobOutcome::Failed {
                items.push(Attention {
                    what: j.title.clone(),
                    why: "protocol run failed".to_string(),
                    source: "proto".to_string(),
                    severity: Severity::High,
                });
            }
        }
        for (name, f) in &self.freshness.sections {
            if *f == Freshness::Unavailable {
                items.push(Attention {
                    what: name.to_string(),
                    why: "unsupported feed version".to_string(),
                    source: "workstate".to_string(),
                    severity: Severity::High,
                });
            }
        }
        items.sort_by_key(|a| std::cmp::Reverse(a.severity));
        items
    }

    /// The full source-confidence list (shown in the Feeds view and, collapsed,
    /// on the busy default screens).
    pub fn source_marks(&self) -> Vec<SourceMark> {
        build_source_marks(
            &self.freshness,
            self.rexops.as_ref(),
            &self.bulwark,
            &self.binaries,
        )
    }
}

/// Test-only representative readings: a built snapshot, one critical bulwark
/// finding, a mix of present/absent rexops sources, and all binaries present.
/// Shared so both the app navigation tests and the `crate::view` draw snapshots
/// exercise the same non-trivial data without touching disk.
#[cfg(test)]
pub(crate) fn sample_readings() -> Readings {
    Readings {
        freshness: SnapshotFreshness {
            built_at: Some("2026-06-14T12:00:00Z".to_string()),
            sections: vec![("scripts", Freshness::Current)],
        },
        rexops: Some(RexopsView {
            generated_at: Some("2026-06-14T12:00:00Z".to_string()),
            sources: vec![
                ("workstate".to_string(), true),
                ("scriptvault".to_string(), false),
            ],
            attention: vec![Attention {
                what: "deploy-prod.sh".to_string(),
                why: "AWS access key ID detected".to_string(),
                source: "bulwark".to_string(),
                severity: Severity::Critical,
            }],
        }),
        bulwark: BulwarkView {
            attention: Vec::new(),
            present: true,
        },
        jobs: Vec::new(),
        binaries: ["workstate", "bulwark", "proto", "toolfoundry", "vault"]
            .iter()
            .map(|&name| BinaryCheck {
                name,
                present: true,
            })
            .collect(),
        now: Some(0),
    }
}

impl Verdict {
    /// Compose the verdict from already-gathered [`Readings`] — the single read
    /// the interactive app reuses for both the verdict and the detail views.
    /// Never fails: missing data degrades to Incomplete, never a panic or an
    /// error screen.
    pub fn from_readings(r: &Readings) -> Self {
        Self::compose(
            r.freshness.clone(),
            r.rexops.clone(),
            r.bulwark.clone(),
            r.jobs.clone(),
            r.binaries.clone(),
            r.now,
        )
    }

    /// The pure core of `build`: given already-read inputs and the current time,
    /// decide the state and fill the screen model. Split out so it is directly
    /// testable without touching the filesystem or the clock.
    fn compose(
        freshness: SnapshotFreshness,
        rexops: Option<RexopsView>,
        bulwark: BulwarkView,
        jobs: Vec<Job>,
        binaries: Vec<BinaryCheck>,
        now: Option<i64>,
    ) -> Self {
        // ---- Attention items, merged across producers ---------------------
        // RexOps is the aggregator; when present its items already cover
        // Bulwark/ToolFoundry. Bulwark's own feed is a fallback for when RexOps
        // isn't running, so we only fold it in if RexOps gave us nothing.
        let mut attention: Vec<Attention> = Vec::new();
        if let Some(rx) = &rexops {
            attention.extend(rx.attention.iter().map(clone_attention));
        }
        if attention.is_empty() {
            attention.extend(bulwark.attention.iter().map(clone_attention));
        }
        // A failed Proto job is its own kind of attention item.
        for j in &jobs {
            if j.outcome == JobOutcome::Failed {
                attention.push(Attention {
                    what: j.title.clone(),
                    why: "protocol run failed".to_string(),
                    source: "proto".to_string(),
                    severity: Severity::High,
                });
            }
        }
        // An unsupported-version section in the snapshot is a real finding too.
        for (name, f) in &freshness.sections {
            if *f == Freshness::Unavailable {
                attention.push(Attention {
                    what: name.to_string(),
                    why: "unsupported feed version".to_string(),
                    source: "workstate".to_string(),
                    severity: Severity::High,
                });
            }
        }

        // Sort the most severe first so the two cause rows show what matters most.
        attention.sort_by_key(|a| std::cmp::Reverse(a.severity));
        let critical = attention
            .iter()
            .filter(|a| a.severity == Severity::Critical)
            .count();
        let high = attention
            .iter()
            .filter(|a| a.severity == Severity::High)
            .count();

        // ---- Source confidence -------------------------------------------
        let sources = build_source_marks(&freshness, rexops.as_ref(), &bulwark, &binaries);
        let unavailable = sources
            .iter()
            .filter(|s| s.freshness == Source::Missing)
            .count();
        let stale = sources
            .iter()
            .filter(|s| s.freshness == Source::Stale)
            .count();
        let snapshot_present = !freshness.sections.is_empty();
        let any_stale =
            freshness.any_stale() || sources.iter().any(|s| s.freshness == Source::Stale);

        // ---- State decision (precedence: attention > incomplete > healthy)-
        let state = if !attention.is_empty() {
            State::NeedsAttention
        } else if !snapshot_present || unavailable > 0 || any_stale {
            State::Incomplete
        } else {
            State::Healthy
        };

        // Confidence is "reduced" when something erodes trust without being a
        // hard finding: stale feeds, or missing sources alongside real findings.
        let confidence_reduced = any_stale || (state == State::NeedsAttention && unavailable > 0);

        // ---- Cause rows (only for NeedsAttention) ------------------------
        let causes = if state == State::NeedsAttention {
            attention
                .iter()
                .take(3) // renderer shows 2, 3 on tall terminals
                .map(|a| Cause {
                    what: a.what.clone(),
                    why: a.why.clone(),
                    source: a.source.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };

        // ---- Age line ----------------------------------------------------
        // Prefer the snapshot's build time; fall back to RexOps's generated_at.
        let stamp = freshness
            .built_at
            .as_deref()
            .or_else(|| rexops.as_ref().and_then(|r| r.generated_at.as_deref()));
        let age = relative_age(stamp, now);

        Verdict {
            state,
            age,
            critical,
            high,
            confidence_reduced,
            unavailable,
            stale,
            causes,
            // Healthy hides the source line entirely (the renderer also guards
            // this), so only carry marks when they have something to say.
            sources: if state == State::Healthy {
                Vec::new()
            } else {
                sources
            },
        }
    }

    /// The demo healthy screen, shown by `--state healthy`.
    pub fn demo_healthy() -> Self {
        Verdict {
            state: State::Healthy,
            age: "2m ago".to_string(),
            critical: 0,
            high: 0,
            confidence_reduced: false,
            unavailable: 0,
            stale: 0,
            causes: Vec::new(),
            sources: Vec::new(),
        }
    }

    /// Demo data for a named state, so all three layouts can be shown via
    /// `--state` without any feeds on disk. `None` for an unknown name.
    pub fn demo(name: &str) -> Option<Self> {
        let mark = |n: &str, f: Source| SourceMark {
            name: n.to_string(),
            freshness: f,
        };
        let cause = |w: &str, y: &str, s: &str| Cause {
            what: w.to_string(),
            why: y.to_string(),
            source: s.to_string(),
        };
        match name {
            "healthy" => Some(Self::demo_healthy()),
            "attention" => Some(Verdict {
                state: State::NeedsAttention,
                age: "2m ago".to_string(),
                critical: 2,
                high: 4,
                confidence_reduced: true,
                unavailable: 1,
                stale: 1,
                causes: vec![
                    cause("deploy-prod.sh", "token-like secret", "bulwark"),
                    cause("findings", "unsupported feed version", "workstate"),
                ],
                sources: vec![
                    mark("workstate", Source::Current),
                    mark("bulwark", Source::Current),
                    mark("toolfoundry", Source::Stale),
                    mark("vault", Source::Missing),
                ],
            }),
            "incomplete" => Some(Verdict {
                state: State::Incomplete,
                age: "2m ago".to_string(),
                critical: 0,
                high: 0,
                confidence_reduced: false,
                unavailable: 2,
                stale: 0,
                causes: Vec::new(),
                sources: vec![
                    mark("workstate", Source::Current),
                    mark("bulwark", Source::Current),
                    mark("toolfoundry", Source::Missing),
                    mark("vault", Source::Missing),
                ],
            }),
            _ => None,
        }
    }
}

/// Build the source-confidence marks shown on non-healthy states. Order is the
/// suite's canonical roster. Freshness comes from the best signal available:
/// the snapshot's own sections, then RexOps's presence map, then whether the
/// producer's binary is even installed.
fn build_source_marks(
    freshness: &SnapshotFreshness,
    rexops: Option<&RexopsView>,
    bulwark: &BulwarkView,
    binaries: &[BinaryCheck],
) -> Vec<SourceMark> {
    // Canonical display roster.
    const ROSTER: &[&str] = &["workstate", "bulwark", "proto", "toolfoundry", "vault"];

    ROSTER
        .iter()
        .map(|&name| {
            let freshness = source_freshness(name, freshness, rexops, bulwark, binaries);
            SourceMark {
                name: name.to_string(),
                freshness,
            }
        })
        .collect()
}

/// Resolve one source's freshness from all available signals.
fn source_freshness(
    name: &str,
    snap: &SnapshotFreshness,
    rexops: Option<&RexopsView>,
    bulwark: &BulwarkView,
    binaries: &[BinaryCheck],
) -> Source {
    // 1. Direct feed freshness wins where Pulse has direct evidence. Installed
    // binaries are intentionally not freshness: a producer on PATH does not mean
    // its data contract is present or current.
    if name == "workstate" {
        return match snap.worst() {
            Some(f) => f.into(),
            None => Source::Missing, // no snapshot at all
        };
    }
    if name == "bulwark" {
        return if bulwark.present {
            Source::Current
        } else {
            Source::Missing
        };
    }

    // RexOps and the installed-binary roster both name this tool "scriptvault";
    // only Pulse's own roster says "vault". Map once, up front, so BOTH the
    // rexops lookup and the binary fallback below use the real name — otherwise
    // the fallback's `b.name == name` never matches and an installed scriptvault
    // with rexops absent is misreported as Missing instead of Stale.
    let key = if name == "vault" { "scriptvault" } else { name };

    // 2. RexOps's aggregated presence map is the best available signal for
    // sources Pulse does not read directly.
    if let Some(rx) = rexops {
        if let Some((_, present)) = rx.sources.iter().find(|(n, _)| n == key) {
            return if *present {
                Source::Current
            } else {
                Source::Missing
            };
        }
    }

    match binaries.iter().find(|b| b.name == key) {
        Some(b) if b.present => Source::Current,
        _ => Source::Missing,
    }
}

use crate::sources::Job;

fn clone_attention(a: &Attention) -> Attention {
    Attention {
        what: a.what.clone(),
        why: a.why.clone(),
        source: a.source.clone(),
        severity: a.severity,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Relative time  (std-only RFC3339 → "2m ago")
// ─────────────────────────────────────────────────────────────────────────────

/// Current time as Unix seconds, or `None` if the clock is before the epoch
/// (it won't be) — used so `compose` can be tested with a fixed clock.
fn unix_now() -> Option<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

/// Format the age of `stamp` (an RFC3339 timestamp like `2026-06-14T12:00:00Z`,
/// or a bare `YYYY-MM-DD`) relative to `now`. Returns a calm short form: "just
/// now", "2m ago", "3h ago", "5d ago". Falls back to "—" when there is no
/// usable timestamp, so the screen always has its one quiet trailing mark.
fn relative_age(stamp: Option<&str>, now: Option<i64>) -> String {
    let (Some(stamp), Some(now)) = (stamp, now) else {
        return "—".to_string();
    };
    let Some(then) = parse_rfc3339_secs(stamp) else {
        return "—".to_string();
    };
    let secs = (now - then).max(0);
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}

/// Parse an RFC3339 / `YYYY-MM-DD` timestamp into Unix seconds (UTC). Tolerant
/// and dependency-free: it understands the subset the suite actually emits
/// (`Z`-suffixed UTC or a bare date) and returns `None` on anything else, which
/// the caller renders as "—". Not a general RFC3339 parser — just enough to age
/// a suite timestamp without pulling chrono.
fn parse_rfc3339_secs(s: &str) -> Option<i64> {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() < 10 {
        return None;
    }
    // Date part: YYYY-MM-DD
    let year: i64 = s.get(0..4)?.parse().ok()?;
    if bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-') {
        return None;
    }
    let month: i64 = s.get(5..7)?.parse().ok()?;
    let day: i64 = s.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    // Optional time part: "T" or " " then HH:MM[:SS].
    let (mut hh, mut mm, mut ss) = (0i64, 0i64, 0i64);
    if bytes.len() >= 16 && (bytes[10] == b'T' || bytes[10] == b' ') {
        hh = s.get(11..13)?.parse().ok()?;
        mm = s.get(14..16)?.parse().ok()?;
        if bytes.len() >= 19 && bytes[16] == b':' {
            ss = s.get(17..19)?.parse().ok()?;
        }
        if hh > 23 || mm > 59 || ss > 60 {
            return None;
        }
    }

    Some(days_from_civil(year, month, day) * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// Days since 1970-01-01 for a civil (proleptic Gregorian) date. Howard
/// Hinnant's well-known constant-time algorithm; handles leap years correctly
/// without a calendar library.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::{Attention, BinaryCheck, BulwarkView, Job, JobOutcome, Severity};

    fn no_rexops() -> Option<RexopsView> {
        None
    }
    fn empty_bulwark() -> BulwarkView {
        BulwarkView {
            attention: Vec::new(),
            present: false,
        }
    }
    fn all_binaries_present() -> Vec<BinaryCheck> {
        // The installed binary is named "scriptvault" (matching rexops and PATH);
        // only Pulse's display roster says "vault".
        [
            "workstate",
            "bulwark",
            "proto",
            "toolfoundry",
            "scriptvault",
        ]
        .iter()
        .map(|&name| BinaryCheck {
            name,
            present: true,
        })
        .collect()
    }
    /// A fully-current snapshot covering the three sections.
    fn fresh_snapshot() -> SnapshotFreshness {
        SnapshotFreshness {
            built_at: Some("2026-06-14T12:00:00Z".to_string()),
            sections: vec![
                ("scripts", Freshness::Current),
                ("tools", Freshness::Current),
                ("findings", Freshness::Current),
            ],
        }
    }

    #[test]
    fn vault_binary_present_without_rexops_is_current_not_missing() {
        // M4 regression: "vault" is Pulse's roster name; the installed binary
        // and rexops both call it "scriptvault". With rexops absent, the binary
        // fallback must map the name and find the installed scriptvault, yielding
        // Current (producer present on PATH) — not Missing.
        let binaries = vec![BinaryCheck {
            name: "scriptvault",
            present: true,
        }];
        let got = source_freshness(
            "vault",
            &fresh_snapshot(),
            None, // rexops down
            &empty_bulwark(),
            &binaries,
        );
        assert_eq!(
            got,
            Source::Current,
            "installed scriptvault must read as Current"
        );

        // And when it is genuinely not installed, it is Missing.
        let none_installed: Vec<BinaryCheck> = Vec::new();
        let got = source_freshness(
            "vault",
            &fresh_snapshot(),
            None,
            &empty_bulwark(),
            &none_installed,
        );
        assert_eq!(got, Source::Missing);
    }

    #[test]
    fn no_data_at_all_is_incomplete() {
        let v = Verdict::compose(
            SnapshotFreshness {
                built_at: None,
                sections: Vec::new(),
            },
            no_rexops(),
            empty_bulwark(),
            Vec::new(),
            // no binaries installed either
            ["workstate", "bulwark", "proto", "toolfoundry", "vault"]
                .iter()
                .map(|&name| BinaryCheck {
                    name,
                    present: false,
                })
                .collect(),
            Some(0),
        );
        assert_eq!(v.state, State::Incomplete);
        assert!(v.unavailable > 0);
        assert!(v.causes.is_empty());
    }

    #[test]
    fn everything_current_and_quiet_is_healthy() {
        // RexOps present, all sources up, no attention.
        let rx = RexopsView {
            generated_at: Some("2026-06-14T12:00:00Z".to_string()),
            sources: vec![
                ("workstate".to_string(), true),
                ("bulwark".to_string(), true),
                ("proto".to_string(), true),
                ("toolfoundry".to_string(), true),
                ("scriptvault".to_string(), true),
            ],
            attention: Vec::new(),
        };
        let v = Verdict::compose(
            fresh_snapshot(),
            Some(rx),
            BulwarkView {
                attention: Vec::new(),
                present: true,
            },
            Vec::new(),
            all_binaries_present(),
            Some(parse_rfc3339_secs("2026-06-14T12:02:00Z").unwrap()),
        );
        assert_eq!(v.state, State::Healthy);
        assert!(v.sources.is_empty(), "healthy hides the source line");
        assert_eq!(v.age, "2m ago");
        assert!(!v.confidence_reduced);
    }

    #[test]
    fn a_critical_finding_forces_needs_attention_even_with_missing_sources() {
        let rx = RexopsView {
            generated_at: Some("2026-06-14T12:00:00Z".to_string()),
            sources: vec![
                ("workstate".to_string(), true),
                ("scriptvault".to_string(), false), // missing
            ],
            attention: vec![Attention {
                what: "deploy-prod.sh".to_string(),
                why: "AWS access key ID detected".to_string(),
                source: "bulwark".to_string(),
                severity: Severity::Critical,
            }],
        };
        let v = Verdict::compose(
            fresh_snapshot(),
            Some(rx),
            empty_bulwark(),
            Vec::new(),
            all_binaries_present(),
            Some(parse_rfc3339_secs("2026-06-14T12:00:30Z").unwrap()),
        );
        assert_eq!(v.state, State::NeedsAttention);
        assert_eq!(v.critical, 1);
        assert_eq!(v.causes.first().unwrap().what, "deploy-prod.sh");
        // a missing source alongside a finding reduces confidence
        assert!(v.confidence_reduced);
        assert_eq!(v.age, "just now");
    }

    #[test]
    fn failed_proto_job_is_attention() {
        let v = Verdict::compose(
            fresh_snapshot(),
            no_rexops(),
            BulwarkView {
                attention: Vec::new(),
                present: true,
            },
            vec![
                Job {
                    title: "Release Readiness".to_string(),
                    outcome: JobOutcome::Failed,
                },
                Job {
                    title: "Rust Review".to_string(),
                    outcome: JobOutcome::Passed,
                },
            ],
            all_binaries_present(),
            Some(parse_rfc3339_secs("2026-06-14T12:00:00Z").unwrap()),
        );
        assert_eq!(v.state, State::NeedsAttention);
        assert_eq!(v.high, 1);
        assert!(v
            .causes
            .iter()
            .any(|c| c.what == "Release Readiness" && c.source == "proto"));
    }

    #[test]
    fn stale_section_makes_the_verdict_incomplete() {
        let mut snap = fresh_snapshot();
        snap.sections[1].1 = Freshness::Stale; // tools stale
        let rx = RexopsView {
            generated_at: Some("2026-06-14T12:00:00Z".to_string()),
            sources: vec![
                ("workstate".to_string(), true),
                ("bulwark".to_string(), true),
                ("proto".to_string(), true),
                ("toolfoundry".to_string(), true),
                ("scriptvault".to_string(), true),
            ],
            attention: Vec::new(),
        };
        let v = Verdict::compose(
            snap,
            Some(rx),
            BulwarkView {
                attention: Vec::new(),
                present: true,
            },
            Vec::new(),
            all_binaries_present(),
            Some(parse_rfc3339_secs("2026-06-14T13:00:00Z").unwrap()),
        );
        assert_eq!(v.state, State::Incomplete);
        assert_eq!(v.stale, 1);
        assert!(v
            .sources
            .iter()
            .any(|s| { s.name == "workstate" && s.freshness == Source::Stale }));
        assert_eq!(v.age, "1h ago");
    }

    #[test]
    fn installed_binary_counts_as_a_current_feed() {
        // As of "Stale -> Current for present binaries": with every producer's
        // binary on PATH and a fresh snapshot, the installed binaries read Current
        // (not Stale), so the suite reads Healthy. A healthy verdict hides the
        // per-source line, so assert the resulting state.
        let v = Verdict::compose(
            fresh_snapshot(),
            no_rexops(),
            BulwarkView {
                attention: Vec::new(),
                present: true,
            },
            Vec::new(),
            all_binaries_present(),
            Some(parse_rfc3339_secs("2026-06-14T12:02:00Z").unwrap()),
        );
        assert_eq!(v.state, State::Healthy);
    }

    #[test]
    fn unsupported_section_becomes_a_finding() {
        let mut snap = fresh_snapshot();
        snap.sections[2].1 = Freshness::Unavailable; // findings unsupported
        let v = Verdict::compose(
            snap,
            no_rexops(),
            BulwarkView {
                attention: Vec::new(),
                present: true,
            },
            Vec::new(),
            all_binaries_present(),
            Some(parse_rfc3339_secs("2026-06-15T12:00:00Z").unwrap()),
        );
        assert_eq!(v.state, State::NeedsAttention);
        assert!(v.causes.iter().any(|c| c.why == "unsupported feed version"));
        assert_eq!(v.age, "1d ago");
    }

    #[test]
    fn relative_age_formats_each_bucket() {
        let base = parse_rfc3339_secs("2026-06-14T12:00:00Z").unwrap();
        let at = |s: &str| parse_rfc3339_secs(s).unwrap();
        assert_eq!(
            relative_age(Some("2026-06-14T12:00:00Z"), Some(base + 10)),
            "just now"
        );
        assert_eq!(
            relative_age(
                Some("2026-06-14T12:00:00Z"),
                Some(at("2026-06-14T12:05:00Z"))
            ),
            "5m ago"
        );
        assert_eq!(
            relative_age(
                Some("2026-06-14T12:00:00Z"),
                Some(at("2026-06-14T15:00:00Z"))
            ),
            "3h ago"
        );
        assert_eq!(
            relative_age(
                Some("2026-06-14T12:00:00Z"),
                Some(at("2026-06-16T12:00:00Z"))
            ),
            "2d ago"
        );
        assert_eq!(relative_age(None, Some(base)), "—");
        assert_eq!(relative_age(Some("not-a-date"), Some(base)), "—");
    }

    #[test]
    fn bare_date_timestamp_parses() {
        // YYYY-MM-DD with no time component (the contract allows this form).
        let d = parse_rfc3339_secs("2026-06-14").unwrap();
        let next = parse_rfc3339_secs("2026-06-15").unwrap();
        assert_eq!(next - d, 86_400);
    }

    #[test]
    fn days_from_civil_matches_known_epochs() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1970, 1, 2), 1);
        assert_eq!(days_from_civil(2000, 1, 1), 10_957); // 30 years, incl. leaps
        assert_eq!(days_from_civil(2026, 6, 14), 20_618);
    }
}
