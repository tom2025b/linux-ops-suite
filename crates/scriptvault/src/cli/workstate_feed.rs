// cli/workstate_feed.rs — emit ScriptVault's versioned Workstate feed.
// -----------------------------------------------------------------------------
// Workstate compiles a suite-wide snapshot by ingesting a small, versioned JSON
// envelope from each tool. This subcommand produces ScriptVault's envelope from
// the LIVE index (a real scan of the configured roots), so the snapshot reflects
// the scripts that exist right now — not a committed fixture. It is the preferred
// Workstate input; `scriptvault search --format json` remains the general scan
// report and is a DIFFERENT (flat, envelope-less) shape.
//
// The envelope shape is fixed by Workstate's `ScriptVaultRaw` adapter:
//   { schema_version, source_tool, generated_at, scripts[{id,name,description}],
//     favorites[<id>], recents[<id>] }
// `generated_at` is stamped at run time, so a fresh run is genuinely fresh — that
// is the whole point: re-running clears Workstate/Conductor's staleness flag
// because the data really was just observed.

use std::path::Path;

use anyhow::{Context, Result};
use clap::Args;
use scriptvault_core::{Config, ScriptEntry, ScriptVault};
use serde::Serialize;

/// The schema version Workstate's scriptvault adapter currently supports. Bump in
/// lockstep with the adapter's `supported_schema_version()` if the shape changes.
const SCHEMA_VERSION: i64 = 1;

/// Arguments for `scriptvault workstate-feed`.
#[derive(Debug, Args)]
pub struct WorkstateFeedArgs {
    /// Write the feed to PATH instead of stdout. The parent directory is created
    /// if missing. Stdout is the default so it composes in a pipe
    /// (`scriptvault workstate-feed | …`); Workstate itself spawns this command
    /// and reads stdout, so no path is needed in the normal flow.
    #[arg(long, value_name = "PATH")]
    pub output: Option<std::path::PathBuf>,

    /// Scan only this directory instead of the configured roots. Repeatable.
    /// Mirrors `search --root`: lets a caller (and the tests) point at a fixture
    /// tree without touching the user's config.
    #[arg(long = "root", value_name = "DIR")]
    pub roots: Vec<std::path::PathBuf>,

    /// Override the `generated_at` stamp (RFC3339) instead of using the current
    /// time. Only for reproducible output in tests/CI; the normal path stamps now.
    #[arg(long, value_name = "RFC3339")]
    pub generated_at: Option<String>,
}

/// The Workstate feed envelope. Serialized straight to JSON; field names and
/// order are the wire contract Workstate's `ScriptVaultRaw` reads.
#[derive(Debug, Serialize)]
struct Feed {
    schema_version: i64,
    source_tool: &'static str,
    generated_at: String,
    scripts: Vec<FeedScript>,
    favorites: Vec<String>,
    recents: Vec<String>,
}

/// One script as the envelope carries it: a stable id plus the human fields.
/// `description` is omitted when absent rather than emitted as `""`, keeping
/// "genuinely no description" honest (Workstate models it as `Option`).
#[derive(Debug, Serialize)]
struct FeedScript {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

/// Derive a stable id for a script from its path's file stem (e.g.
/// `/s/deploy-prod.sh` → `deploy-prod`). The stem is stable across runs and
/// matches the existing feed convention. Falls back to the full filename, then
/// the path string, so the id is NEVER blank — Workstate drops blank-id records,
/// so a missing id would silently lose the script.
fn script_id(path: &Path) -> String {
    path.file_stem()
        .or_else(|| path.file_name())
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Build the feed from a loaded ScriptVault facade. Pure (no I/O): takes the live
/// engine and a timestamp, returns the envelope. Favorites/recents are emitted as
/// the SAME ids as `scripts`, mapped from their on-disk paths, so a consumer can
/// cross-reference them; ids whose script is no longer indexed are dropped.
fn build_feed(sv: &ScriptVault, generated_at: String) -> Feed {
    // id() by path, so favorites/recents (path-keyed in state) map to the same
    // id space as the scripts list. Only ids that correspond to a currently
    // indexed script are kept — a stale favorite to a deleted file is dropped.
    let entries: Vec<&ScriptEntry> = sv.all().iter().collect();

    let known: std::collections::HashSet<String> =
        entries.iter().map(|e| script_id(&e.path)).collect();

    let scripts = entries
        .iter()
        .map(|e| FeedScript {
            id: script_id(&e.path),
            name: e.display_name().to_string(),
            description: e.meta.desc.clone(),
        })
        .collect();

    let favorites = entries
        .iter()
        .filter(|e| sv.is_favorite(&e.path))
        .map(|e| script_id(&e.path))
        .collect();

    // Recents are newest-first in state; keep that order, map to ids, and drop any
    // that no longer resolve to an indexed script.
    let recents = sv
        .recents()
        .iter()
        .map(|r| script_id(&r.path))
        .filter(|id| known.contains(id))
        .collect();

    Feed {
        schema_version: SCHEMA_VERSION,
        source_tool: "scriptvault",
        generated_at,
        scripts,
        favorites,
        recents,
    }
}

/// Run `scriptvault workstate-feed` end-to-end: load the engine, build the feed,
/// and write it to `--output` (or stdout). A trailing newline is appended so the
/// file/pipe ends cleanly.
pub fn run(args: WorkstateFeedArgs) -> Result<()> {
    // Quiet logging: this is a data command whose stdout may BE the feed (Workstate
    // reads it from a pipe), so diagnostics must stay on stderr and not be chatty.
    crate::logging::init_cli(false);

    let sv = if args.roots.is_empty() {
        ScriptVault::load()?
    } else {
        let mut config = Config::load().or_else(|_| Config::defaults())?;
        config.roots = args.roots.clone();
        ScriptVault::load_with(config)?
    };

    // Stamp now (UTC RFC3339) unless overridden. This is what makes a fresh run
    // read as Fresh downstream — the timestamp is observed at emit time.
    let generated_at = args
        .generated_at
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    let feed = build_feed(&sv, generated_at);
    // Pretty JSON: the feed is read by humans during debugging, and Workstate's
    // parser is whitespace-insensitive, so prettiness costs nothing.
    let json = serde_json::to_string_pretty(&feed).context("serializing workstate feed")?;

    match args.output {
        Some(path) => {
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            std::fs::write(&path, format!("{json}\n"))
                .with_context(|| format!("writing feed to {}", path.display()))?;
            eprintln!("scriptvault: wrote workstate feed -> {}", path.display());
        }
        None => println!("{json}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// A throwaway fixture tree with two annotated scripts, unique per test.
    fn fixture_tree() -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("scriptvault-wsfeed-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("deploy-prod.sh"),
            "#!/bin/bash\n# scriptvault.name: deploy-prod.sh\n# scriptvault.desc: ship it\necho a\n",
        )
        .unwrap();
        fs::write(
            dir.join("backup-db.sh"),
            "#!/bin/bash\n# scriptvault.desc: nightly backup\necho b\n",
        )
        .unwrap();
        dir
    }

    fn load_at(dir: &Path) -> ScriptVault {
        let mut config = Config::defaults().unwrap();
        config.roots = vec![dir.to_path_buf()];
        ScriptVault::load_with(config).unwrap()
    }

    #[test]
    fn script_id_is_the_file_stem_and_never_blank() {
        assert_eq!(script_id(Path::new("/s/deploy-prod.sh")), "deploy-prod");
        assert_eq!(script_id(Path::new("backup-db.py")), "backup-db");
        // A pathological path with no stem still yields a non-blank id.
        assert!(!script_id(Path::new("/")).trim().is_empty());
    }

    #[test]
    fn feed_has_the_envelope_workstate_expects() {
        let dir = fixture_tree();
        let sv = load_at(&dir);
        let feed = build_feed(&sv, "2026-06-22T00:00:00Z".to_string());

        // Envelope identity is the contract Workstate's adapter gates on.
        assert_eq!(feed.schema_version, 1);
        assert_eq!(feed.source_tool, "scriptvault");
        assert_eq!(feed.generated_at, "2026-06-22T00:00:00Z");

        // Both fixtures are present, each with a stable, non-blank id.
        assert_eq!(feed.scripts.len(), 2);
        let ids: Vec<&str> = feed.scripts.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&"deploy-prod"));
        assert!(ids.contains(&"backup-db"));
        assert!(feed.scripts.iter().all(|s| !s.id.trim().is_empty()));

        // The explicit-name script shows its name; the unnamed one falls back to
        // the filename (display_name()), so the column is never empty.
        let deploy = feed.scripts.iter().find(|s| s.id == "deploy-prod").unwrap();
        assert_eq!(deploy.name, "deploy-prod.sh");
        assert_eq!(deploy.description.as_deref(), Some("ship it"));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn feed_serializes_and_reparses_as_the_workstate_shape() {
        let dir = fixture_tree();
        let sv = load_at(&dir);
        let feed = build_feed(&sv, "2026-06-22T00:00:00Z".to_string());
        let json = serde_json::to_string_pretty(&feed).unwrap();

        // The envelope round-trips and exposes exactly the keys Workstate reads.
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["source_tool"], "scriptvault");
        assert!(v["generated_at"].is_string());
        assert!(v["scripts"].is_array());
        assert!(v["favorites"].is_array());
        assert!(v["recents"].is_array());
        // A script object carries id/name; description is present when set.
        let scripts = v["scripts"].as_array().unwrap();
        assert!(
            scripts
                .iter()
                .all(|s| s["id"].is_string() && s["name"].is_string())
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn description_is_omitted_when_absent_not_emitted_as_empty() {
        // The unnamed fixture HAS a desc; make one with truly no desc to prove the
        // skip. A bare file with no header has no metadata description.
        let dir = fixture_tree();
        fs::write(dir.join("plain.sh"), "#!/bin/sh\necho plain\n").unwrap();
        let sv = load_at(&dir);
        let feed = build_feed(&sv, "t".to_string());
        let plain = feed.scripts.iter().find(|s| s.id == "plain").unwrap();
        assert!(
            plain.description.is_none(),
            "a script with no description must omit the field, not send \"\""
        );
        // And the serialized form genuinely lacks the key.
        let json = serde_json::to_value(plain).unwrap();
        assert!(json.get("description").is_none());

        fs::remove_dir_all(&dir).ok();
    }
}
