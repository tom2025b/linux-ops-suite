// ============================================================================
// crates/scriptvault-core/tests/pipeline.rs  —  END-TO-END INTEGRATION TESTS
// ----------------------------------------------------------------------------
// PURPOSE
//   These tests exercise the WHOLE ScriptVault pipeline — config -> scan -> parse
//   -> index -> search — through ONLY the crate's public API. Files in a
//   crate's `tests/` directory are compiled as separate crates that link
//   against `scriptvault_core` as an external dependency, so (unlike the in-module
//   `#[cfg(test)] mod tests` unit tests) they can reach NOTHING private. That
//   constraint is the point: if these pass, the public surface
//   (`ScriptVault::load_with`, `.search`, `.all`, and the re-exported data types)
//   is genuinely enough to drive the engine from a frontend (CLI/TUI/GUI).
//
//   The unit tests inside `src/**` already cover each STAGE in isolation
//   (header parsing, sidecar merge, language inference, fuzzy ranking). We do
//   NOT re-test those here — we test that the stages compose correctly on a
//   real on-disk directory tree.
//
// DELIBERATE NON-GOALS
//   • We never exec/run a script (no `scriptvault_core::actions::run`). Spawning a
//     freshly-written+chmod'd file races with the kernel and can raise ETXTBSY
//     ("text file busy"); that path is covered by unit tests. We stay on the
//     pure filesystem-read + in-memory side of the pipeline.
//   • No external test crates (no `tempfile`, no `assert_cmd`). Only `std`.
//
// KEY PIPELINE FACTS these tests rely on (verified by reading the source):
//   • `ScriptVault::load_with(cfg)` does NOT merge the shipped default.yaml — it
//     scans `cfg.roots` using EXACTLY `cfg.ignores`. So each fixture Config
//     must list its own ignores (e.g. ".git"), or those files get indexed.
//   • `*.scriptvault.yaml` sidecars are skipped by the scanner; they annotate a
//     sibling script and never become entries themselves.
//   • A readable file with no annotations is still indexed (findable by
//     filename) — "unannotated is never invisible".
//   • Ranking is TIERED: field rank (Name > Tags > Desc > Filename) is the
//     PRIMARY key; raw fuzzy score only breaks ties WITHIN a tier. And the
//     `Name` match field fires ONLY when an explicit `name` is set — a bare
//     filename hit is labeled `Filename`, never `Name`.
//   • Sidecar YAML uses BARE serde field names (`name:`, `desc:`), whereas the
//     inline header uses `# scriptvault.name:` comment lines.
// ============================================================================

// Bring in the PUBLIC API only. This is the entire vocabulary a frontend has.
use scriptvault_core::{Config, Language, MatchField, MetaSource, ScriptVault};

// `std::fs` for building/cleaning the fixture tree; `std::path` for paths;
// `std::time` to make each temp dir name unique per run.
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Test helpers (shared across the scenarios below).
// ---------------------------------------------------------------------------

/// Create a unique, throwaway fixture root under the OS temp dir.
///
/// The `tag` (a per-TEST string) is what prevents collisions between tests
/// running in PARALLEL — two tests must never share a directory name, or one's
/// `remove_dir_all` could delete files the other is still scanning, causing
/// flaky failures. The nanosecond timestamp additionally guards against a
/// stale directory left over from a previous run of the SAME test.
fn fixture_root(tag: &str) -> PathBuf {
    // `SystemTime::now()` minus the Unix epoch gives a duration we read as
    // nanoseconds — a cheap, monotonic-enough unique suffix for a dir name.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // temp_dir() is the platform temp location (/tmp on Linux). We combine it
    // with our crate tag, the per-test tag, and the timestamp.
    let root = std::env::temp_dir().join(format!("scriptvault-it-{tag}-{nanos}"));
    // create_dir_all is idempotent and makes any missing parents in one call.
    fs::create_dir_all(&root).expect("create fixture root");
    root
}

/// Write `contents` to `path`, creating parent directories as needed.
///
/// We use plain `fs::write` (no executable bit, no chmod): the whole pipeline
/// only ever READS files as text, so a script's "runnability" is irrelevant to
/// scanning, parsing, and indexing. Avoiding chmod also keeps us nowhere near
/// the ETXTBSY exec race the task warns about.
fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, contents).expect("write fixture file");
}

/// Build a `Config` that scans exactly `root` with the given ignore fragments.
///
/// IMPORTANT: `ScriptVault::load_with` uses these `ignores` VERBATIM — it does not
/// fold in the shipped defaults. Every test that creates an ignored directory
/// (like `.git`) must therefore list it here, or the scanner will happily index
/// the files inside it (they are perfectly readable text).
fn config_for(root: &Path, ignores: &[&str]) -> Config {
    Config {
        roots: vec![root.to_path_buf()],
        ignores: ignores.iter().map(|s| s.to_string()).collect(),
        // `None` editor is fine: we never invoke the "open in editor" action.
        editor: None,
    }
}

/// Find the single indexed entry whose filename equals `name`.
///
/// Panics (failing the test) if there is not exactly one — a clear signal that
/// the fixture or the pipeline produced something unexpected.
fn entry_by_filename<'a>(sk: &'a ScriptVault, name: &str) -> &'a scriptvault_core::ScriptEntry {
    let matches: Vec<_> = sk.all().iter().filter(|e| e.filename == name).collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one entry named {name:?}, found {}",
        matches.len()
    );
    matches[0]
}

// ===========================================================================
// SCENARIO 1 — a realistic MIXED tree: full header, sidecar override,
// unannotated file, a sidecar file itself, a nested subdir, and a pruned dir.
// ---------------------------------------------------------------------------
// Locks the core "shape" guarantees of scan+parse together:
//   • a fully-annotated script is parsed (source = Header);
//   • a sidecar OVERRIDES the header (sidecar wins; source = Both);
//   • an unannotated script is STILL indexed (findable by filename);
//   • a `*.scriptvault.yaml` file does NOT appear as its own entry;
//   • a script in a NESTED subdirectory is discovered (recursion works);
//   • a file inside an ignored dir (`.git`) is PRUNED (never indexed).
// ===========================================================================
#[test]
fn mixed_tree_scan_parse_shape() {
    let root = fixture_root("mixed");

    // (a) Fully-annotated script with a complete inline header. No sidecar, so
    //     its provenance must resolve to MetaSource::Header.
    write_file(
        &root.join("backup-db.sh"),
        "#!/usr/bin/env bash\n\
         # scriptvault.name: Backup Postgres\n\
         # scriptvault.desc: Dumps the prod database with a timestamp\n\
         # scriptvault.tags: db, backup, postgres\n\
         # scriptvault.category: database\n\
         echo backing up\n",
    );

    // (b) A script with BOTH a header and a sidecar. The sidecar overrides the
    //     name; the header's desc has no sidecar counterpart, so it survives.
    //     Provenance must be MetaSource::Both, and display_name the sidecar's.
    write_file(
        &root.join("deploy.sh"),
        "#!/usr/bin/env bash\n\
         # scriptvault.name: HeaderDeployName\n\
         # scriptvault.desc: Ships the app to prod\n\
         echo deploying\n",
    );
    // Sidecar uses BARE serde keys (not `# scriptvault.*`). It wins on `name`.
    write_file(
        &root.join("deploy.sh.scriptvault.yaml"),
        "name: SidecarDeployName\ncategory: ops\n",
    );

    // (c) A completely unannotated script — no header, no sidecar. Must still
    //     be indexed and findable by its filename ("never invisible").
    write_file(&root.join("plain.sh"), "#!/bin/bash\necho plain\n");

    // (d) A script in a NESTED subdirectory — proves recursive discovery.
    write_file(
        &root.join("nested/sub/inner.sh"),
        "#!/bin/bash\n# scriptvault.name: Inner Tool\necho inner\n",
    );

    // (e) A file inside an ignored `.git` directory — must be PRUNED. Note it
    //     is readable text, so ONLY the ignore rule keeps it out of the index.
    write_file(
        &root.join(".git/hooks/pre-commit"),
        "#!/bin/bash\necho hook\n",
    );

    // Build the engine over this tree, ignoring `.git` (we must say so — see
    // the helper's note; load_with does not apply default ignores).
    let sk = ScriptVault::load_with(config_for(&root, &[".git"])).expect("load_with");

    // Collect all indexed filenames for convenient membership assertions.
    let filenames: Vec<&str> = sk.all().iter().map(|e| e.filename.as_str()).collect();

    // (a) The annotated script is present with header-only provenance.
    let backup = entry_by_filename(&sk, "backup-db.sh");
    assert_eq!(backup.display_name(), "Backup Postgres");
    assert_eq!(backup.source, MetaSource::Header);
    assert_eq!(backup.lang, Language::Bash); // from the bash shebang

    // (b) Sidecar wins on name; header desc survives; provenance is Both.
    let deploy = entry_by_filename(&sk, "deploy.sh");
    assert_eq!(deploy.display_name(), "SidecarDeployName");
    assert_eq!(deploy.meta.desc.as_deref(), Some("Ships the app to prod"));
    assert_eq!(deploy.meta.category.as_deref(), Some("ops"));
    assert_eq!(deploy.source, MetaSource::Both);

    // (c) The unannotated script is indexed and falls back to its filename.
    let plain = entry_by_filename(&sk, "plain.sh");
    assert_eq!(plain.display_name(), "plain.sh");
    assert_eq!(plain.source, MetaSource::None);
    // ...and it is genuinely FINDABLE BY FILENAME via search (not merely present
    // in `all()`). With no name/desc/tags, the only matchable field is the
    // filename, so the hit must be labeled MatchField::Filename.
    assert!(
        sk.search("plain")
            .iter()
            .any(|r| r.entry.filename == "plain.sh" && r.matched_field == MatchField::Filename),
        "an unannotated script must be findable by its filename"
    );

    // (d) The nested script was discovered by the recursive walk.
    assert!(
        filenames.contains(&"inner.sh"),
        "nested subdir must be scanned"
    );

    // (e) NOTHING under `.git` is indexed (the whole subtree is pruned).
    assert!(
        !sk.all()
            .iter()
            .any(|e| e.path.components().any(|c| c.as_os_str() == ".git")),
        "files inside an ignored .git dir must be pruned"
    );

    // The sidecar FILE must not be indexed as its own entry — only the script.
    assert!(
        !filenames.iter().any(|f| f.ends_with(".scriptvault.yaml")),
        "a *.scriptvault.yaml sidecar must not appear as its own entry"
    );

    // Exactly the four real scripts: backup-db.sh, deploy.sh, plain.sh, inner.sh.
    assert_eq!(sk.all().len(), 4, "expected exactly the 4 real scripts");

    fs::remove_dir_all(&root).ok();
}

// ===========================================================================
// SCENARIO 2 — tiered ranking, end to end: a NAME match must outrank a DESC
// match, and the winning result's matched_field must be MatchField::Name.
// ---------------------------------------------------------------------------
// This is the discriminating ranking test, driven through the real pipeline.
// The trap (confirmed in index::score_entry): per entry we keep the
// HIGHEST-SCORING field and only prefer the higher-priority field on a score
// TIE. So the query must be a fuzzy subsequence of the intended field ONLY —
// crucially NOT of either filename, or a filename hit could outscore the name
// hit and silently relabel the result `Filename`. We therefore use filenames
// (`alpha.sh`, `bravo.sh`) that do NOT contain "deploy" as a subsequence.
// ===========================================================================
#[test]
fn tiered_ranking_name_beats_desc() {
    let root = fixture_root("ranking");

    // Entry A — matches the query "deploy" via an explicit NAME only. Its
    // filename ("alpha.sh") deliberately does NOT contain "deploy" so the only
    // possible match is the Name field.
    write_file(
        &root.join("alpha.sh"),
        "#!/bin/bash\n# scriptvault.name: deploy\necho a\n",
    );

    // Entry B — matches via a DESCRIPTION only (no explicit name). Filename
    // ("bravo.sh") again has no "deploy" subsequence, so the only match is Desc.
    write_file(
        &root.join("bravo.sh"),
        "#!/bin/bash\n# scriptvault.desc: deploy deploy deployment deploy\necho b\n",
    );

    let sk = ScriptVault::load_with(config_for(&root, &[".git"])).expect("load_with");

    let results = sk.search("deploy");
    // Both scripts match the query — one by name, one by description.
    assert_eq!(results.len(), 2, "both scripts should match 'deploy'");

    // Tiered rule: the NAME match ranks first regardless of raw score magnitude.
    assert_eq!(
        results[0].matched_field,
        MatchField::Name,
        "a name match must rank above a desc match"
    );
    assert_eq!(results[0].entry.display_name(), "deploy");

    // The runner-up is the description match.
    assert_eq!(results[1].matched_field, MatchField::Desc);
    assert_eq!(results[1].entry.filename, "bravo.sh");

    fs::remove_dir_all(&root).ok();
}

// ===========================================================================
// SCENARIO 3 — an EMPTY query returns ALL indexed entries.
// ---------------------------------------------------------------------------
// `search("")` (and whitespace) is the "show everything" path the TUI uses on
// first open. Count must equal the readable, non-sidecar scripts NOT under an
// ignored dir. We give this test its OWN small, obvious fixture so the count is
// unambiguous: 3 scripts + 1 sidecar (skipped) + 1 .git file (pruned) = 3.
// ===========================================================================
#[test]
fn empty_query_returns_all_entries() {
    let root = fixture_root("all");

    // Three real scripts (one annotated, one with a sidecar, one bare).
    write_file(
        &root.join("one.sh"),
        "#!/bin/bash\n# scriptvault.name: One\necho 1\n",
    );
    write_file(&root.join("two.py"), "#!/usr/bin/env python3\nprint(2)\n");
    write_file(&root.join("three.sh"), "#!/bin/bash\necho 3\n");
    // A sidecar for `one.sh` — annotates, never counts as its own entry.
    write_file(&root.join("one.sh.scriptvault.yaml"), "category: misc\n");
    // A pruned file under `.git`.
    write_file(&root.join(".git/config"), "[core]\n");

    let sk = ScriptVault::load_with(config_for(&root, &[".git"])).expect("load_with");

    // `all()` and an empty `search` must agree on the full set's SIZE.
    assert_eq!(sk.all().len(), 3, "three real scripts should be indexed");

    // Empty query => everything. Whitespace-only must behave identically.
    let empty = sk.search("");
    let blank = sk.search("   ");
    assert_eq!(empty.len(), 3, "empty query returns all entries");
    assert_eq!(blank.len(), 3, "whitespace query returns all entries");

    // The empty-query result set must contain exactly the three scripts.
    let mut names: Vec<&str> = empty.iter().map(|r| r.entry.filename.as_str()).collect();
    names.sort();
    assert_eq!(names, vec!["one.sh", "three.sh", "two.py"]);

    fs::remove_dir_all(&root).ok();
}

// ===========================================================================
// SCENARIO 4 — sidecar-wins merge, end to end.
// ---------------------------------------------------------------------------
// Header declares name = "HeaderName" and desc = "HeaderDesc". The sidecar
// declares name = "SidecarName" (and a new category) but NO desc. After the
// pipeline merges them: display_name() == "SidecarName" (sidecar overrides the
// conflicting field) while the header-only desc SURVIVES (sidecar didn't touch
// it). Provenance is Both. This locks "sidecar wins on conflict, header-only
// fields persist" through the public API.
// ===========================================================================
#[test]
fn sidecar_wins_but_header_only_field_survives() {
    let root = fixture_root("sidecar");

    write_file(
        &root.join("task.sh"),
        "#!/bin/bash\n\
         # scriptvault.name: HeaderName\n\
         # scriptvault.desc: HeaderDesc\n\
         echo task\n",
    );
    // Sidecar overrides `name`, adds `category`, but says nothing about `desc`.
    write_file(
        &root.join("task.sh.scriptvault.yaml"),
        "name: SidecarName\ncategory: ops\n",
    );

    let sk = ScriptVault::load_with(config_for(&root, &[".git"])).expect("load_with");
    let task = entry_by_filename(&sk, "task.sh");

    // Sidecar value won on the conflicting `name` field.
    assert_eq!(task.display_name(), "SidecarName");
    // Header-only `desc` survived the merge (sidecar didn't provide one).
    assert_eq!(task.meta.desc.as_deref(), Some("HeaderDesc"));
    // The sidecar's new field is present too.
    assert_eq!(task.meta.category.as_deref(), Some("ops"));
    // Both sources contributed -> Both.
    assert_eq!(task.source, MetaSource::Both);

    fs::remove_dir_all(&root).ok();
}

// ===========================================================================
// SCENARIO 5 — language inference, end to end.
// ---------------------------------------------------------------------------
// Two facts at once, through the pipeline:
//   • A `.py` file with a python shebang resolves to Language::Python (shebang
//     /extension inference with no explicit override).
//   • A file whose header declares `# scriptvault.lang: bash` resolves to
//     Language::Bash even though its shebang/extension say python — explicit
//     metadata.lang wins over inferred language. (We give it a `.py` extension
//     and python shebang precisely so the override is doing real work.)
// ===========================================================================
#[test]
fn language_inference_and_explicit_override() {
    let root = fixture_root("lang");

    // Inferred: python shebang + .py extension, no explicit lang.
    write_file(
        &root.join("infer.py"),
        "#!/usr/bin/env python3\n# scriptvault.name: Inferred Py\nprint('hi')\n",
    );

    // Overridden: looks like python (shebang + .py) but declares lang: bash.
    // The header is still PARSED using python's `#` comment leader, but the
    // resolved language must be Bash because explicit lang wins.
    write_file(
        &root.join("override.py"),
        "#!/usr/bin/env python3\n# scriptvault.lang: bash\n# scriptvault.name: Overridden\nprint('x')\n",
    );

    let sk = ScriptVault::load_with(config_for(&root, &[".git"])).expect("load_with");

    let inferred = entry_by_filename(&sk, "infer.py");
    assert_eq!(
        inferred.lang,
        Language::Python,
        "python shebang/ext -> Python"
    );

    let overridden = entry_by_filename(&sk, "override.py");
    assert_eq!(
        overridden.lang,
        Language::Bash,
        "explicit `# scriptvault.lang: bash` overrides the python shebang/ext"
    );
    // The override file's header was still parsed (name came through), proving
    // the python comment leader was used to READ it before relabeling its lang.
    assert_eq!(overridden.display_name(), "Overridden");

    fs::remove_dir_all(&root).ok();
}

// ===========================================================================
// SCENARIO 6 — reload() picks up a newly-added script.
// ---------------------------------------------------------------------------
// Load against a fixture with N scripts; assert count N. Write a brand-new
// script into the same root; call reload(); assert count N+1 and that the new
// script is now findable. This locks the "re-scan on demand" contract: reload
// rebuilds the index from the current config's roots without re-reading config.
// ===========================================================================
#[test]
fn reload_picks_up_new_script() {
    let root = fixture_root("reload");

    // Start with two scripts.
    write_file(
        &root.join("first.sh"),
        "#!/bin/bash\n# scriptvault.name: First\necho 1\n",
    );
    write_file(
        &root.join("second.sh"),
        "#!/bin/bash\n# scriptvault.name: Second\necho 2\n",
    );

    // `mut` because reload() takes &mut self (it rebuilds the index in place).
    let mut sk = ScriptVault::load_with(config_for(&root, &[".git"])).expect("load_with");
    assert_eq!(sk.all().len(), 2, "two scripts present at load time");

    // Add a THIRD script to the same root AFTER the initial load.
    write_file(
        &root.join("third.sh"),
        "#!/bin/bash\n# scriptvault.name: Third\necho 3\n",
    );

    // Before reload, the index is stale and must still report 2.
    assert_eq!(sk.all().len(), 2, "index is stale until reload() is called");

    // Re-scan: the new file is discovered without touching the config.
    sk.reload().expect("reload");
    assert_eq!(sk.all().len(), 3, "reload() must discover the new script");

    // And the new script is now searchable by its explicit name.
    let hits = sk.search("Third");
    assert!(
        hits.iter().any(|r| r.entry.filename == "third.sh"),
        "the newly-added script must be findable after reload"
    );

    fs::remove_dir_all(&root).ok();
}

// ===========================================================================
// SCENARIO 7 — reload() picks up sidecar changes (script untouched). `reload()`
// re-reads every file, so a Bulwark-written sidecar edit/delete is reflected.
// ===========================================================================

#[test]
fn reload_picks_up_a_sidecar_only_edit() {
    // The script is untouched; only its sidecar changes. The entry must still
    // update — the bridge writes sidecars, and reload re-reads them.
    let root = fixture_root("reload-sidecar-edit");
    write_file(
        &root.join("svc.sh"),
        "#!/bin/bash\n# scriptvault.name: HeaderName\necho x\n",
    );

    let mut sk = ScriptVault::load_with(config_for(&root, &[".git"])).expect("load_with");
    assert_eq!(
        entry_by_filename(&sk, "svc.sh").display_name(),
        "HeaderName"
    );

    // Write a sidecar that overrides the name; the SCRIPT file is NOT touched.
    write_file(
        &root.join("svc.sh.scriptvault.yaml"),
        "name: SidecarName\ntags: [risk:high]\n",
    );
    sk.reload().expect("reload");

    let svc = entry_by_filename(&sk, "svc.sh");
    assert_eq!(svc.display_name(), "SidecarName");
    assert_eq!(svc.source, MetaSource::Both);
    assert!(svc.meta.tags.iter().any(|t| t == "risk:high"));

    fs::remove_dir_all(&root).ok();
}

#[test]
fn reload_reverts_when_sidecar_is_deleted() {
    // Mirror case: deleting the sidecar (script untouched) drops back to
    // header-only metadata.
    let root = fixture_root("reload-sidecar-del");
    write_file(
        &root.join("svc.sh"),
        "#!/bin/bash\n# scriptvault.name: HeaderName\necho x\n",
    );
    let sidecar = root.join("svc.sh.scriptvault.yaml");
    write_file(&sidecar, "name: SidecarName\n");

    let mut sk = ScriptVault::load_with(config_for(&root, &[".git"])).expect("load_with");
    assert_eq!(
        entry_by_filename(&sk, "svc.sh").display_name(),
        "SidecarName"
    );
    assert_eq!(entry_by_filename(&sk, "svc.sh").source, MetaSource::Both);

    fs::remove_file(&sidecar).unwrap();
    sk.reload().expect("reload");

    let svc = entry_by_filename(&sk, "svc.sh");
    assert_eq!(svc.display_name(), "HeaderName");
    assert_eq!(svc.source, MetaSource::Header);

    fs::remove_dir_all(&root).ok();
}
