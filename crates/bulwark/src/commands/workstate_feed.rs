//! `bulwark workstate-feed` command implementation.
//!
//! Split out of `main.rs` to keep the binary entry point thin: `main` owns CLI
//! parsing and dispatch, while the per-command logic (scan → render → publish)
//! lives in its own focused module. This is the producer side of Bulwark's
//! versioned Workstate feed integration; the envelope itself is rendered by
//! `bulwark_core::render_workstate_feed`.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};

use bulwark::{ClassifiedEntry, Config};

use crate::{print_scan_warnings, warn_missing_scan_paths};

/// Run `bulwark workstate-feed`: scan, render the v1 feed envelope, and either
/// print it to stdout or publish it atomically to `--output`.
pub fn run(
    cli_paths: Vec<String>,
    generated_at: Option<String>,
    output: Option<PathBuf>,
) -> Result<()> {
    let mut config = Config::load().context("failed to load configuration")?;

    if !cli_paths.is_empty() {
        config.scan.paths = cli_paths;
    }

    // Validate any user-supplied timestamp up front, before scanning, so a typo
    // fails fast with a clear message instead of silently publishing a feed with
    // a malformed `generated_at` that downstream Workstate consumers can't parse.
    let generated_at = match generated_at {
        Some(value) => {
            DateTime::parse_from_rfc3339(&value).with_context(|| {
                format!(
                    "invalid --generated-at {value:?}: expected an RFC 3339 timestamp, e.g. 2026-06-06T12:00:00Z"
                )
            })?;
            value
        }
        None => current_utc_timestamp(),
    };

    warn_missing_scan_paths(&config)?;

    let mut classified =
        bulwark::collect_classified_inventory(&config).context("failed to scan and classify")?;

    if let Some(output) = output.as_deref() {
        exclude_output_path(&mut classified.entries, output);
    }

    print_scan_warnings(&classified.warnings);

    let json = bulwark::render_workstate_feed(&classified.entries, &generated_at)
        .context("failed to render Workstate feed")?;

    match output {
        Some(output) => write_atomic(&output, &json)?,
        None => println!("{json}"),
    }

    Ok(())
}

/// Drop the entry (if any) that refers to the feed's own `--output` file.
///
/// Publishing to a path that sits inside a scanned directory would otherwise
/// make the feed include its own (stale) previous version on the next run — a
/// self-referential entry that is never what the user wants. We compare
/// canonicalized paths so this holds regardless of how the output path was
/// spelled (relative, symlink, `./`, etc.); the output may not exist yet, so
/// `canonical_or_self` falls back to the literal path when it can't be resolved.
fn exclude_output_path(entries: &mut Vec<ClassifiedEntry>, output: &Path) {
    let target = canonical_or_self(output);
    entries.retain(|entry| canonical_or_self(&entry.entry.discovered.path) != target);
}

/// Current UTC time as a seconds-precision RFC 3339 timestamp (e.g.
/// `2026-06-06T12:00:00Z`), used when the caller doesn't pass `--generated-at`.
fn current_utc_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Canonicalize `path`, falling back to the path as-given when it can't be
/// resolved (e.g. it doesn't exist yet). Used to compare the output target
/// against scanned entries without being fooled by spelling differences.
fn canonical_or_self(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Write `contents` to `path` atomically: write to a temp file in the same
/// directory, then rename over the destination. The rename is atomic on POSIX,
/// so readers never observe a half-written feed.
fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }

    let temp_path = temp_output_path(path);
    write_private(&temp_path, contents)
        .with_context(|| format!("writing temporary feed {}", temp_path.display()))?;

    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error).with_context(|| format!("publishing feed {}", path.display()));
    }

    Ok(())
}

/// Write `contents` to a fresh temp `path`, creating it with owner-only (0600)
/// permissions and failing if it already exists.
///
/// The feed can describe security-relevant inventory, so we don't want it
/// readable by other local users. On Unix we set the mode at create time via
/// `OpenOptions` so the file is never briefly world-readable.
///
/// We use `create_new` (O_EXCL) rather than `create().truncate()`: the temp
/// path lives in the user-supplied `--output` directory, which may be shared or
/// attacker-influenced. With plain create+truncate, a pre-planted symlink at the
/// temp name would make us follow it and clobber an arbitrary user-writable file
/// before the rename (a local symlink/TOCTOU race). O_EXCL refuses to open when
/// the path already exists and never follows a final symlink, closing that race.
/// Combined with a randomized temp suffix, the name can't be predicted and
/// pre-created either.
#[cfg(unix)]
fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(contents.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(contents.as_bytes())
}

/// Build a hidden, unpredictable temp path next to the destination so the atomic
/// rename stays within the same filesystem (rename across filesystems is not
/// atomic and can fail with `EXDEV`).
///
/// The suffix mixes the PID with a high-resolution timestamp so the name can't
/// be guessed and pre-created by another local user; paired with the
/// `create_new` (O_EXCL) open in [`write_private`], this defeats the local
/// symlink race on the temp file.
fn temp_output_path(path: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    // Process-unique, monotonically increasing counter so repeated calls never
    // collide even within the same clock tick.
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("bulwark-workstate-feed");

    let time_nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);

    parent.join(format!(
        ".{file_name}.{}.{time_nonce:x}.{seq:x}.tmp",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bulwark::{Classification, DiscoveredFile, Language, RiskLevel, ScriptEntry};

    /// Build a minimal `ClassifiedEntry` at `path` for exclusion tests. Field
    /// values besides the path are irrelevant here — exclusion keys only on path.
    fn entry_at(path: &str) -> ClassifiedEntry {
        ClassifiedEntry {
            entry: ScriptEntry {
                discovered: DiscoveredFile {
                    path: PathBuf::from(path),
                    size: 0,
                    is_executable: false,
                },
                language: Language::Bash,
                description: None,
                sidecar: None,
                sidecar_warning: None,
            },
            classification: Classification {
                risk: RiskLevel::Low,
                category: "script".to_string(),
                owner: "user".to_string(),
            },
        }
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_creates_file_with_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feed.json");

        write_atomic(&path, "{}").unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "newly created feed must be owner-only (0600)");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_overwrite_keeps_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feed.json");

        // Pre-create the destination with permissive 0644 to prove the atomic
        // rewrite replaces it with a fresh 0600 file rather than inheriting the
        // old, looser mode.
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        write_atomic(&path, "new").unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "rewritten feed must be 0600, not the old 0644");
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn exclude_output_path_drops_target_when_file_does_not_exist_yet() {
        // First run: the --output file has not been created. The path won't
        // canonicalize, so exclusion must fall back to literal-path matching.
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("bulwark.json");
        assert!(!output.exists());

        let other = dir.path().join("alpha.sh");
        let mut entries = vec![
            entry_at(output.to_str().unwrap()),
            entry_at(other.to_str().unwrap()),
        ];

        exclude_output_path(&mut entries, &output);

        let remaining: Vec<&PathBuf> = entries.iter().map(|e| &e.entry.discovered.path).collect();
        assert_eq!(
            remaining,
            vec![&other],
            "output path must be excluded; others kept"
        );
    }

    #[test]
    fn exclude_output_path_drops_target_on_rerun_when_file_exists() {
        // Re-run: the --output file already exists from a previous run. Both the
        // output and the scanned entry canonicalize to the same real path, so it
        // must be excluded even though it physically exists.
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("bulwark.json");
        fs::write(&output, "{}").unwrap();
        assert!(output.exists());

        let other = dir.path().join("beta.sh");
        fs::write(&other, "x").unwrap();

        let mut entries = vec![
            entry_at(output.to_str().unwrap()),
            entry_at(other.to_str().unwrap()),
        ];

        exclude_output_path(&mut entries, &output);

        let remaining: Vec<&PathBuf> = entries.iter().map(|e| &e.entry.discovered.path).collect();
        assert_eq!(
            remaining,
            vec![&other],
            "existing output path must still be excluded"
        );
    }

    #[test]
    fn write_private_refuses_preexisting_path() {
        // O_EXCL (create_new) is the symlink-race guard: if the temp path
        // already exists, the open must fail rather than follow/truncate it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".feed.tmp");
        fs::write(&path, "planted").unwrap();

        let err = write_private(&path, "new").unwrap_err();
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::AlreadyExists,
            "write_private must refuse an existing temp path (O_EXCL)"
        );
        // The planted file is untouched.
        assert_eq!(fs::read_to_string(&path).unwrap(), "planted");
    }

    #[test]
    fn temp_output_path_is_unpredictable_across_calls() {
        // The temp suffix mixes PID + a nanosecond nonce, so two calls for the
        // same destination produce different names (can't be pre-planted).
        let dest = Path::new("/tmp/out/bulwark.json");
        let a = temp_output_path(dest);
        let b = temp_output_path(dest);
        assert_ne!(a, b, "temp path should differ between calls");
        assert_eq!(a.parent(), dest.parent(), "temp stays in the dest dir");
    }

    #[test]
    fn exclude_output_path_keeps_entries_when_no_match() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("bulwark.json");
        let a = dir.path().join("a.sh");
        let b = dir.path().join("b.sh");

        let mut entries = vec![entry_at(a.to_str().unwrap()), entry_at(b.to_str().unwrap())];
        exclude_output_path(&mut entries, &output);

        assert_eq!(entries.len(), 2, "no entry matches the output; all kept");
    }
}
