//! The recursive directory walk. Given one [`WatchEntry`], produce every
//! filesystem path it covers: the path itself, and — for a recursive directory
//! — everything beneath it that isn't excluded. The walk is iterative (an
//! explicit stack, no recursion) with a depth guard, never follows a symlinked
//! directory back into the tree unless `follow_symlinks` is set, and prunes
//! excluded names so a `.git`/`*.log` exclusion costs nothing to descend.

use std::fs;
use std::path::PathBuf;

use crate::watch::{glob_match, WatchEntry};

/// A guard against runaway descent (deeply nested or symlink-looped trees).
/// 64 levels is far past any real config tree; hitting it stops that branch
/// quietly rather than spinning.
const MAX_DEPTH: usize = 64;

/// Collect every path covered by one watch entry, in stable sorted order.
///
/// - A file or symlink contributes just itself.
/// - A non-recursive directory contributes itself plus its immediate children.
/// - A recursive directory contributes itself and the full subtree, pruning any
///   name or full path matching an `exclude` glob.
///
/// Symlinked directories are descended into only when `follow_symlinks` is set;
/// otherwise the symlink is recorded as a leaf (its own entry) and not followed,
/// which is the safe default for an integrity tool. A path that can't be read
/// (permission denied on a dir listing) simply contributes itself and stops —
/// degradation, not error.
pub fn collect(entry: &WatchEntry) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let root = entry.path.clone();

    // The root itself is always part of the set (if it exists at all — that's
    // checked by the scanner via its metadata; here we include the path and let
    // the scanner decide existence).
    out.push(root.clone());

    // Only directories expand. Use lstat: a symlink-to-dir is a symlink leaf
    // unless following is requested.
    let md = match fs::symlink_metadata(&root) {
        Ok(m) => m,
        Err(_) => return out, // missing/unreadable root: just the path itself
    };

    let is_dir = if md.file_type().is_symlink() {
        entry.follow_symlinks && root.is_dir()
    } else {
        md.is_dir()
    };
    if !is_dir {
        return out;
    }

    // Stack of (dir, depth). Depth 0 is the root's children.
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.clone(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let rd = match fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(_) => continue, // unreadable dir: skip its children, keep going
        };
        for child in rd.flatten() {
            let path = child.path();
            let name = child.file_name().to_string_lossy().into_owned();
            let full = path.to_string_lossy();

            if is_excluded(&entry.exclude, &name, &full) {
                continue;
            }
            out.push(path.clone());

            // Descend into subdirectories when recursive and within depth.
            if !entry.recursive || depth + 1 >= MAX_DEPTH {
                continue;
            }
            let cmd = match fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let descend = if cmd.file_type().is_symlink() {
                entry.follow_symlinks && path.is_dir()
            } else {
                cmd.is_dir()
            };
            if descend {
                stack.push((path, depth + 1));
            }
        }
    }

    out.sort();
    out.dedup();
    out
}

/// Whether a child should be pruned: any exclude glob matching either the bare
/// file name or the full path.
fn is_excluded(excludes: &[String], name: &str, full: &str) -> bool {
    excludes
        .iter()
        .any(|pat| glob_match(pat, name) || glob_match(pat, full))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn touch(p: &std::path::Path) {
        fs::write(p, b"x").unwrap();
    }

    #[test]
    fn single_file_yields_only_itself() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("a.txt");
        touch(&f);
        let e = WatchEntry::new(f.clone());
        assert_eq!(collect(&e), vec![f]);
    }

    #[test]
    fn missing_path_yields_only_itself() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("nope");
        let e = WatchEntry::new(f.clone());
        assert_eq!(collect(&e), vec![f]);
    }

    #[test]
    fn recursive_dir_collects_whole_subtree() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        touch(&dir.path().join("top.txt"));
        touch(&sub.join("deep.txt"));

        let e = WatchEntry::new(dir.path().to_path_buf());
        let got = collect(&e);
        assert!(got.contains(&dir.path().to_path_buf()));
        assert!(got.contains(&dir.path().join("top.txt")));
        assert!(got.contains(&sub));
        assert!(got.contains(&sub.join("deep.txt")));
    }

    #[test]
    fn non_recursive_dir_stops_at_immediate_children() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        touch(&dir.path().join("top.txt"));
        touch(&sub.join("deep.txt"));

        let mut e = WatchEntry::new(dir.path().to_path_buf());
        e.recursive = false;
        let got = collect(&e);
        assert!(got.contains(&dir.path().join("top.txt")));
        assert!(got.contains(&sub)); // the subdir entry is listed
        assert!(!got.contains(&sub.join("deep.txt"))); // but not its contents
    }

    #[test]
    fn excludes_prune_by_name_and_are_not_descended() {
        let dir = tempdir().unwrap();
        let git = dir.path().join(".git");
        fs::create_dir(&git).unwrap();
        touch(&git.join("HEAD"));
        touch(&dir.path().join("keep.txt"));
        touch(&dir.path().join("skip.log"));

        let mut e = WatchEntry::new(dir.path().to_path_buf());
        e.exclude = vec![".git".to_string(), "*.log".to_string()];
        let got = collect(&e);
        assert!(got.contains(&dir.path().join("keep.txt")));
        assert!(!got.iter().any(|p| p.starts_with(&git))); // pruned, not descended
        assert!(!got.contains(&dir.path().join("skip.log")));
    }

    #[test]
    fn symlinked_dir_not_followed_by_default() {
        let dir = tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        touch(&real.join("inside.txt"));
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let mut e = WatchEntry::new(dir.path().to_path_buf());
        e.recursive = true;
        let got = collect(&e);
        // The symlink itself is listed...
        assert!(got.contains(&link));
        // ...but we did not walk through it into real's contents *via the link*.
        assert!(!got.contains(&link.join("inside.txt")));
    }
}
