use std::{
    env,
    path::{Path, PathBuf},
};

use crate::{
    error::{Result, ToolFoundryError},
    manifest::ToolManifest,
};

/// Expand manifest paths that use `~` or `~/` into absolute home paths.
///
/// Only the current user's home is supported. `~otheruser` syntax is rejected
/// explicitly rather than silently treated as a literal relative path, which
/// would resolve install and link targets to a surprising location.
pub fn expand_manifest_path(path: &str) -> Result<PathBuf> {
    if path == "~" {
        return home_dir(path);
    }

    if let Some(rest) = path.strip_prefix("~/") {
        return Ok(home_dir(path)?.join(rest));
    }

    if path.starts_with('~') {
        return Err(ToolFoundryError::PathExpansion {
            path: path.to_string(),
            reason: "~user expansion is not supported; use ~ or an absolute path".to_string(),
        });
    }

    Ok(PathBuf::from(path))
}

/// Display a path relative to the manifest owner path when possible.
pub fn display_relative_to_manifest(path: &Path, manifest: &ToolManifest) -> String {
    let root = Path::new(&manifest.ownership.local_path);
    path.strip_prefix(root).map_or_else(
        |_| path.display().to_string(),
        |value| value.display().to_string(),
    )
}

fn home_dir(original: &str) -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| ToolFoundryError::PathExpansion {
            path: original.to_string(),
            reason: "HOME is not set".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn preserves_absolute_path() {
        let path = expand_manifest_path("/tmp/tool").expect("absolute path should expand");

        assert_eq!(path, PathBuf::from("/tmp/tool"));
    }

    #[test]
    fn expands_tilde_slash_against_home() {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        // Only assert when HOME is set in the test environment; the expansion is then
        // exactly HOME joined with the remainder.
        if let Some(home) = home {
            let path = expand_manifest_path("~/bin/tool").expect("tilde path should expand");
            assert_eq!(path, home.join("bin/tool"));
        }
    }

    #[test]
    fn preserves_relative_path_without_tilde() {
        let path =
            expand_manifest_path("relative/tool").expect("relative path should pass through");

        assert_eq!(path, PathBuf::from("relative/tool"));
    }

    #[test]
    fn rejects_other_user_home_expansion() {
        // `~root/bin` must not silently become the literal relative path "~root/bin";
        // ToolFoundry only resolves the current user's home.
        let error =
            expand_manifest_path("~root/bin").expect_err("~user expansion should be rejected");

        assert!(matches!(error, ToolFoundryError::PathExpansion { .. }));
        assert!(error.to_string().contains("~user"), "message: {error}");
    }
}
