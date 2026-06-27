// scan — walk the configured roots and return candidate script paths (no reads,
// no language guessing; that's the parser). The subtleties that each prevent a
// real bug:
//   • Prune ignored DIRECTORIES as we descend (don't walk .git/ then filter).
//   • Match ignores against an entry's OWN name, not its ancestry — else a root
//     under a dir named like an ignore fragment would silently yield nothing.
//   • Ignores are globs (globset), compiled once; a bad pattern is a clear error.
//   • Classify with `path().is_file()` (follows a final symlink) not
//     `file_type().is_file()` (false for a symlink) — `~/bin` is full of links.
//   • Keep `follow_links(false)` so symlinked DIRS can't loop or escape roots.
//   • Skip `*.scriptvault.yaml` sidecars (metadata, found via their sibling).
//   • Missing roots / per-entry errors are skipped, never fatal; dedup overlaps.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use walkdir::{DirEntry, WalkDir};

use crate::config::Config;
use crate::error::{Result, ScriptVaultError};
use crate::parser::lang;

/// A file ending in this is metadata for a script, not a script itself.
const SIDECAR_SUFFIX: &str = ".scriptvault.yaml";

/// Walk every configured root and return the candidate script paths. Rarely
/// errors: missing roots and unreadable entries are skipped, not raised.
pub fn walk(config: &Config) -> Result<Vec<PathBuf>> {
    let ignores = build_ignore_set(&config.ignores)?;

    // Gather in config order, dedup at the end (nicer discovery order than a
    // dedup-as-we-go HashSet).
    let mut paths: Vec<PathBuf> = Vec::new();

    for root in &config.roots {
        // A nonexistent root yields a single Err entry, dropped below — no check.
        let walker = WalkDir::new(root)
            .follow_links(false) // don't loop on / escape via symlinked dirs
            .into_iter()
            // filter_entry prunes a DIRECTORY without descending into it.
            .filter_entry(|entry| !is_ignored(entry, &ignores));

        for entry in walker {
            let Ok(entry) = entry else { continue }; // skip permission/race errors

            // Candidates are files that LOOK like scripts (known extension or a
            // `#!`). `is_file()` follows a final symlink, so symlinked scripts
            // count and broken links don't; the gate keeps binaries/data files
            // away from the parser.
            if entry.path().is_file() && !is_sidecar(&entry) && looks_like_script(entry.path()) {
                paths.push(entry.into_path());
            }
        }
    }

    Ok(dedup_preserving_order(paths))
}

/// Compile the ignore patterns into a `GlobSet` (matched later against an entry's
/// own name). A plain name matches itself; "*.tmp", "build-*" also work. A
/// pattern that won't compile is a `BadIgnorePattern` error, never silent.
fn build_ignore_set(patterns: &[String]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pat in patterns {
        if pat.trim().is_empty() {
            continue;
        }
        let glob = Glob::new(pat).map_err(|source| ScriptVaultError::BadIgnorePattern {
            pattern: pat.clone(),
            source,
        })?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|source| ScriptVaultError::BadIgnorePattern {
            pattern: patterns.join(", "), // build failure isn't tied to one pattern
            source,
        })
}

/// Ignore test: match the entry's OWN name (last component) against the glob set
/// — never the whole path, so an ignore fragment in a root's ancestry can't wipe
/// it out.
fn is_ignored(entry: &DirEntry, ignores: &GlobSet) -> bool {
    ignores.is_match(entry.file_name())
}

/// A candidate when it has a known script extension OR begins with `#!`. The
/// extension check decides most files for free; only then do we peek for a
/// shebang. The single gate that keeps non-scripts away from the parser.
fn looks_like_script(path: &Path) -> bool {
    lang::is_known_script_ext(path) || has_shebang(path)
}

/// True if the first two bytes are `#!`. Reads bytes not text, so a binary or
/// unreadable file just reports `false`.
fn has_shebang(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = [0u8; 2];
    matches!(file.read(&mut buf), Ok(2)) && &buf == b"#!"
}

/// True if the entry is a `*.scriptvault.yaml` sidecar (metadata, not a script).
fn is_sidecar(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_string_lossy()
        .ends_with(SIDECAR_SUFFIX)
}

/// Remove duplicate paths, keeping first-seen order (overlapping roots like "~"
/// and "~/bin" would otherwise surface a file twice).
fn dedup_preserving_order(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

// Tests — on a real temp directory tree (std only).
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    /// Throwaway tree under the temp dir; unique name per test for parallelism.
    fn make_tree(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let base = std::env::temp_dir().join(format!("scriptvault-scan-{tag}-{nanos}"));
        fs::create_dir_all(&base).unwrap();
        base
    }

    /// Helper: write a file, creating parent dirs as needed.
    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    /// Helper: write raw bytes (e.g. a non-UTF-8 "binary"), creating parents.
    fn write_bytes(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    /// Helper: build a Config that scans exactly `root` with the given ignores.
    fn config_for(root: &Path, ignores: &[&str]) -> Config {
        Config {
            roots: vec![root.to_path_buf()],
            ignores: ignores.iter().map(|s| s.to_string()).collect(),
            editor: None,
        }
    }

    #[test]
    fn finds_files_and_skips_ignored_dirs() {
        let root = make_tree("basic");
        write(&root.join("a.sh"), "#!/bin/bash\n");
        write(&root.join("sub/b.py"), "#!/usr/bin/env python\n");
        // These live under ignored directories and must be pruned.
        write(&root.join(".git/config"), "[core]\n");
        write(&root.join("node_modules/pkg/index.js"), "//x\n");

        let cfg = config_for(&root, &[".git", "node_modules"]);
        let found = walk(&cfg).unwrap();

        let names: HashSet<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert!(names.contains("a.sh"));
        assert!(names.contains("b.py"));
        assert!(!names.contains("config"), "should not descend into .git");
        assert!(!names.contains("index.js"), "should prune node_modules");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn root_nested_under_ignore_fragment_still_scanned() {
        // THE trap: configure a root whose PATH passes through a dir named like
        // an ignore fragment ("target"). The file must still be returned,
        // because ignores match an entry's own NAME, not its ancestry.
        let base = make_tree("nested");
        let root = base.join("target").join("myroot");
        write(&root.join("foo.sh"), "#!/bin/bash\n");

        let cfg = config_for(&root, &["target", ".git", "node_modules"]);
        let found = walk(&cfg).unwrap();

        let names: HashSet<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(
            names.contains("foo.sh"),
            "a configured root under a dir named like an ignore fragment must still scan"
        );

        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn binaries_and_data_files_are_not_candidates() {
        // A file with no script extension and no shebang is not a candidate.
        let root = make_tree("nonscript");
        // A "binary": non-UTF-8 bytes, no extension, no shebang.
        write_bytes(&root.join("blob"), &[0xff, 0xfe, 0x00, 0x80]);
        // A data file with an extension we don't recognize.
        write(&root.join("data.csv"), "a,b,c\n1,2,3\n");
        // A genuine script, to prove we still find the right thing.
        write(&root.join("keep.sh"), "#!/bin/bash\necho hi\n");

        let cfg = config_for(&root, &[]);
        let found = walk(&cfg).unwrap();
        let names: HashSet<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert!(names.contains("keep.sh"), "a real script must be found");
        assert!(!names.contains("blob"), "a binary must not be a candidate");
        assert!(
            !names.contains("data.csv"),
            "an unknown-extension data file must not be a candidate"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn extensionless_shebang_file_is_a_candidate() {
        // The shebang path of the gate: a file with NO extension but a `#!` first
        // line (the classic `~/bin/deploy` case) is still a candidate.
        let root = make_tree("shebang");
        write(&root.join("deploy"), "#!/bin/bash\necho deploy\n");
        // And one without a shebang or known extension is excluded, for contrast.
        write(&root.join("notes"), "just some notes, not a script\n");

        let cfg = config_for(&root, &[]);
        let found = walk(&cfg).unwrap();
        let names: HashSet<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert!(
            names.contains("deploy"),
            "an extensionless file with a #! shebang must be a candidate"
        );
        assert!(
            !names.contains("notes"),
            "a plain text file with no shebang/extension is not a script"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn skips_sidecar_yaml_files() {
        let root = make_tree("sidecar");
        write(&root.join("deploy.sh"), "#!/bin/bash\n");
        // Its sidecar — metadata, not a script. Must not appear as a candidate.
        write(&root.join("deploy.sh.scriptvault.yaml"), "name: Deploy\n");

        let cfg = config_for(&root, &[]);
        let found = walk(&cfg).unwrap();
        let names: HashSet<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert!(names.contains("deploy.sh"));
        assert!(
            !names.iter().any(|n| n.ends_with(".scriptvault.yaml")),
            "sidecars must be excluded from candidates"
        );

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn glob_ignores_prune_files_and_dirs_while_literals_still_work() {
        // Glob patterns now work alongside literal names. "*.bak" should drop a
        // backup file; "build-*" should prune a matching directory; the literal
        // ".git" still prunes as before.
        let root = make_tree("glob");
        write(&root.join("keep.sh"), "#!/bin/bash\n");
        write(&root.join("old.sh.bak"), "#!/bin/bash\n"); // matches *.bak
        write(&root.join("build-2026/out.sh"), "#!/bin/bash\n"); // dir matches build-*
        write(&root.join(".git/config"), "[core]\n"); // literal .git

        let cfg = config_for(&root, &["*.bak", "build-*", ".git"]);
        let found = walk(&cfg).unwrap();
        let names: HashSet<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();

        assert!(names.contains("keep.sh"), "non-matching file must be kept");
        assert!(
            !names.contains("old.sh.bak"),
            "*.bak glob must drop backups"
        );
        assert!(!names.contains("out.sh"), "build-* glob must prune the dir");
        assert!(!names.contains("config"), "literal .git must still prune");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn bad_glob_pattern_is_a_clear_error_not_a_silent_noop() {
        // An unparseable glob must surface as BadIgnorePattern, naming the bad
        // pattern — never silently ignore nothing.
        let root = make_tree("badglob");
        write(&root.join("a.sh"), "#!/bin/bash\n");
        // "a[" is an unterminated character class → invalid glob.
        let cfg = config_for(&root, &["a["]);
        let err = walk(&cfg).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("a["), "error must name the bad pattern: {msg}");

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_root_is_not_fatal() {
        // A root that does not exist yields no entries and no error.
        let cfg = config_for(Path::new("/definitely/not/a/real/scriptvault/path"), &[]);
        let found = walk(&cfg).unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn overlapping_roots_dedup() {
        let root = make_tree("dedup");
        write(&root.join("only.sh"), "#!/bin/bash\n");

        // Same directory listed twice as two roots -> file must appear once.
        let cfg = Config {
            roots: vec![root.clone(), root.clone()],
            ignores: vec![],
            editor: None,
        };
        let found = walk(&cfg).unwrap();
        let count = found
            .iter()
            .filter(|p| p.file_name().unwrap() == "only.sh")
            .count();
        assert_eq!(count, 1, "overlapping roots must not duplicate a file");

        fs::remove_dir_all(&root).ok();
    }

    // NOTE: a symlink test is intentionally omitted from the default suite
    // because creating symlinks is platform-specific; the behavior is covered
    // by using `path().is_file()` (documented above), which follows the link
    // target so symlinked scripts are included while broken links drop out.
}
