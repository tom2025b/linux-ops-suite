//! The ONLY filesystem sink. Persists a FileTree; optionally `git init`s.
use crate::error::WriteError;
use crate::filetree::FileTree;
use std::path::Path;
use std::process::Command;

pub struct WriteOpts {
    pub force: bool,
    pub dry_run: bool,
    pub git: bool,
}

/// Write `tree` under `dest`. In `dry_run`, nothing is written. Unless `force`,
/// `dest` must be absent or empty. With `git`, runs init + add + initial commit.
pub fn write(tree: &FileTree, dest: &Path, opts: &WriteOpts) -> Result<(), WriteError> {
    if opts.dry_run {
        return Ok(());
    }

    if dest.exists() {
        let non_empty = std::fs::read_dir(dest)
            .map_err(|e| WriteError::Io(e.to_string()))?
            .next()
            .is_some();
        if non_empty && !opts.force {
            return Err(WriteError::TargetNotEmpty(dest.display().to_string()));
        }
    }

    for (rel, contents) in tree.iter() {
        let path = dest.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| WriteError::Io(e.to_string()))?;
        }
        std::fs::write(&path, contents).map_err(|e| WriteError::Io(e.to_string()))?;
    }

    if opts.git {
        git_init(dest)?;
    }
    Ok(())
}

fn git_init(dest: &Path) -> Result<(), WriteError> {
    let run = |args: &[&str]| -> Result<(), WriteError> {
        let status = Command::new("git")
            .args(args)
            .current_dir(dest)
            .status()
            .map_err(|e| WriteError::Git(e.to_string()))?;
        if !status.success() {
            return Err(WriteError::Git(format!("git {args:?} failed")));
        }
        Ok(())
    };
    run(&["init", "-q"])?;
    run(&["add", "-A"])?;
    run(&["commit", "-q", "-m", "Initial commit (rex-forge)"])?;
    Ok(())
}
