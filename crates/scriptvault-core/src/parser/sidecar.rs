// ============================================================================
// crates/scriptvault-core/src/parser/sidecar.rs
// ============================================================================
// Sidecar handling + the metadata merge.
//
// A sidecar is `<script-path>.scriptvault.yaml` sitting next to the script. It lets
// a user annotate a script they can't or won't edit (system scripts, vendored
// tools). Per the design decision: SIDECAR WINS over the inline header.
//
// Error policy (decided with the user): a MALFORMED sidecar is NON-FATAL — we
// warn on stderr and degrade to header-only, so one YAML typo never hides an
// otherwise well-annotated script. This module therefore returns
// `Option<ScriptMetadata>`: None means "no usable sidecar" (absent OR broken),
// and the broken case emits a warning as a side effect.
// ============================================================================

use std::path::{Path, PathBuf};

use crate::model::ScriptMetadata;

/// The suffix appended to a script path to find its sidecar.
const SIDECAR_SUFFIX: &str = ".scriptvault.yaml";

/// Compute the sidecar path for a given script path: append the suffix to the
/// FULL filename (so `deploy.sh` -> `deploy.sh.scriptvault.yaml`).
pub fn sidecar_path(script: &Path) -> PathBuf {
    // We build the new filename by string concatenation on the OS string, which
    // correctly preserves the directory and the original extension.
    let mut os = script.as_os_str().to_owned();
    os.push(SIDECAR_SUFFIX);
    PathBuf::from(os)
}

/// Load and parse a script's sidecar metadata, if a usable one exists.
///
/// Returns:
///   • `Some(meta)` if a sidecar file exists and parsed successfully.
///   • `None` if there is no sidecar, OR it exists but is malformed/unreadable
///     (in which case a warning is printed to stderr and we degrade gracefully).
pub fn load_sidecar(script: &Path) -> Option<ScriptMetadata> {
    let path = sidecar_path(script);
    if !path.exists() {
        return None;
    }

    match std::fs::read_to_string(&path) {
        Ok(raw) if raw.trim().is_empty() => {
            // An empty sidecar contributes nothing, but isn't an error.
            None
        }
        Ok(raw) => match serde_yaml::from_str::<ScriptMetadata>(&raw) {
            Ok(meta) => Some(meta),
            Err(err) => {
                // NON-FATAL: warn and degrade to header-only.
                warn_sidecar(&path, &err.to_string());
                None
            }
        },
        Err(err) => {
            warn_sidecar(&path, &err.to_string());
            None
        }
    }
}

/// Emit a non-fatal sidecar warning through `tracing`. WARN level because this
/// is a real, user-actionable problem (a malformed sidecar they probably want to
/// fix) — distinct from the per-file scan skips, which are DEBUG. The binary's
/// subscriber decides where it lands: stderr for the CLI, the log file for the
/// TUI (so it never corrupts the alternate screen). `path` is a structured field
/// so a future machine-readable sink can filter on it.
fn warn_sidecar(path: &Path, reason: &str) {
    tracing::warn!(path = %path.display(), %reason, "ignoring malformed sidecar");
}

/// Merge header metadata with sidecar metadata, SIDECAR WINS on conflict.
///
/// Field-by-field, mirroring the Layer-2 config merge discipline:
///   • Option fields: `sidecar.or(header)` — sidecar value wins when `Some`.
///   • `tags`: sidecar wins ONLY if non-empty; otherwise keep the header's
///     tags. This `!is_empty()` guard prevents a sidecar that sets only, say,
///     `name` (whose `tags` deserialize to `[]` via serde default) from
///     silently wiping the header's tags — the same trap as Layers 2 and 3.
pub fn merge(header: ScriptMetadata, sidecar: ScriptMetadata) -> ScriptMetadata {
    ScriptMetadata {
        name: sidecar.name.or(header.name),
        desc: sidecar.desc.or(header.desc),
        usage: sidecar.usage.or(header.usage),
        category: sidecar.category.or(header.category),
        lang: sidecar.lang.or(header.lang),
        risk: sidecar.risk.or(header.risk),
        owner: sidecar.owner.or(header.owner),
        tags: if !sidecar.tags.is_empty() {
            sidecar.tags
        } else {
            header.tags
        },
    }
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn meta(name: Option<&str>, desc: Option<&str>, tags: &[&str]) -> ScriptMetadata {
        ScriptMetadata {
            name: name.map(str::to_string),
            desc: desc.map(str::to_string),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn sidecar_path_appends_to_full_filename() {
        assert_eq!(
            sidecar_path(Path::new("/x/deploy.sh")),
            PathBuf::from("/x/deploy.sh.scriptvault.yaml")
        );
    }

    #[test]
    fn sidecar_overrides_header_on_conflict() {
        let header = meta(Some("HeaderName"), Some("HeaderDesc"), &["h1"]);
        let side = meta(Some("SideName"), None, &["s1", "s2"]);
        let merged = merge(header, side);
        // Sidecar name + tags win; desc falls back to header (sidecar None).
        assert_eq!(merged.name.as_deref(), Some("SideName"));
        assert_eq!(merged.desc.as_deref(), Some("HeaderDesc"));
        assert_eq!(merged.tags, vec!["s1", "s2"]);
    }

    #[test]
    fn sidecar_setting_only_name_keeps_header_tags_and_desc() {
        // THE trap: sidecar sets only `name`; its empty `tags` must NOT wipe the
        // header's tags, and its `None` desc must NOT wipe the header's desc.
        let header = meta(Some("H"), Some("KeepDesc"), &["keep1", "keep2"]);
        let side = meta(Some("OnlyName"), None, &[]);
        let merged = merge(header, side);
        assert_eq!(merged.name.as_deref(), Some("OnlyName"));
        assert_eq!(merged.desc.as_deref(), Some("KeepDesc"));
        assert_eq!(merged.tags, vec!["keep1", "keep2"]);
    }

    use std::fs;

    /// A unique temp dir for filesystem-backed sidecar tests.
    fn tmp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("scriptvault-sidecar-{tag}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn absent_sidecar_is_none() {
        // No sidecar file at all → None (the common case), no warning, no error.
        let dir = tmp_dir("absent");
        let script = dir.join("a.sh");
        fs::write(&script, "#!/bin/sh\n").unwrap();
        assert!(load_sidecar(&script).is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_sidecar_contributes_nothing() {
        // An empty (whitespace-only) sidecar is not malformed — it just adds
        // nothing, so it loads as None (no warning).
        let dir = tmp_dir("empty");
        let script = dir.join("a.sh");
        fs::write(&script, "#!/bin/sh\n").unwrap();
        fs::write(sidecar_path(&script), "   \n  ").unwrap();
        assert!(load_sidecar(&script).is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn malformed_sidecar_yaml_is_none_not_error() {
        // Broken YAML degrades to None (a warning is printed as a side effect).
        let dir = tmp_dir("malformed");
        let script = dir.join("a.sh");
        fs::write(&script, "#!/bin/sh\n").unwrap();
        fs::write(sidecar_path(&script), "name: [unterminated\n").unwrap();
        assert!(load_sidecar(&script).is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sidecar_with_only_unknown_keys_loads_as_default() {
        // `#[serde(default)]` + unknown keys: serde_yaml ignores fields that
        // aren't on `ScriptMetadata`, so a sidecar of purely foreign keys parses
        // to an all-default metadata (Some, but contributes nothing on merge).
        let dir = tmp_dir("unknown");
        let script = dir.join("a.sh");
        fs::write(&script, "#!/bin/sh\n").unwrap();
        fs::write(sidecar_path(&script), "color: blue\npriority: 9\n").unwrap();
        let loaded = load_sidecar(&script);
        assert_eq!(loaded, Some(ScriptMetadata::default()));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sidecar_partial_fields_only_set_what_is_present() {
        // A sidecar with just `category` leaves every other field None/empty —
        // the partial-config guarantee at the sidecar level.
        let dir = tmp_dir("partial");
        let script = dir.join("a.sh");
        fs::write(&script, "#!/bin/sh\n").unwrap();
        fs::write(sidecar_path(&script), "category: ops\n").unwrap();
        let loaded = load_sidecar(&script).unwrap();
        assert_eq!(loaded.category.as_deref(), Some("ops"));
        assert!(loaded.name.is_none());
        assert!(loaded.tags.is_empty());
        fs::remove_dir_all(&dir).ok();
    }
}
