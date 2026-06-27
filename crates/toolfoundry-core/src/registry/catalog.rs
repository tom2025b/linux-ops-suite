use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    error::{Result, ToolFoundryError},
    manifest::load_manifest,
};

use super::{ManifestCatalog, ManifestSummary};

/// Load and summarize all top-level YAML manifests in a directory.
pub fn load_catalog(directory: impl AsRef<Path>) -> Result<ManifestCatalog> {
    let summaries = manifest_paths(directory)?
        .into_iter()
        .map(|path| {
            let manifest = load_manifest(&path)?;
            Ok(ManifestSummary::from_manifest(path, manifest))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(ManifestCatalog::new(summaries))
}

/// Return deterministic top-level `.yaml` and `.yml` manifest paths.
///
/// An ABSENT registry directory is NOT an error: it means no tools have been
/// registered yet, which yields an empty catalog (and, downstream, an empty
/// Workstate feed) rather than a hard failure. This matches the suite's
/// fail-soft contract — a not-yet-configured ToolFoundry must not break
/// `toolfoundry workstate-feed` (and therefore Workstate's snapshot). Any OTHER
/// read error (permissions, a file where a dir was expected) is still a real
/// `RegistryRead` failure and propagates.
pub fn manifest_paths(directory: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    let directory = directory.as_ref();
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        // No registry dir yet → no manifests. Empty catalog, not an error.
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(source) => {
            return Err(ToolFoundryError::RegistryRead {
                path: directory.to_path_buf(),
                source,
            });
        }
    };

    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| ToolFoundryError::RegistryRead {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();

        if !has_yaml_extension(&path) {
            continue;
        }

        let file_type = entry
            .file_type()
            .map_err(|source| ToolFoundryError::RegistryRead {
                path: path.clone(),
                source,
            })?;
        if file_type.is_file() {
            paths.push(path);
        }
    }

    paths.sort();
    Ok(paths)
}

fn has_yaml_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml")
        })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;
    use crate::test_support::ManifestFixture;

    static NEXT_DIR_ID: AtomicUsize = AtomicUsize::new(0);

    struct TempRegistry {
        path: PathBuf,
    }

    impl TempRegistry {
        fn new() -> Self {
            let id = NEXT_DIR_ID.fetch_add(1, Ordering::Relaxed);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "toolfoundry-registry-test-{}-{nanos}-{id}",
                std::process::id(),
            ));
            fs::create_dir(&path).expect("temp registry directory should be created");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn write(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.path.join(name);
            fs::write(&path, contents).expect("registry fixture should be written");
            path
        }
    }

    impl Drop for TempRegistry {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn catalog_loads_yaml_manifests_and_sorts_by_id() {
        let registry = TempRegistry::new();
        let alpha_path = registry.write("z-alpha.yaml", &manifest("alpha-tool", "Alpha Tool"));
        let beta_path = registry.write("a-beta.yml", &manifest("beta-tool", "Beta Tool"));
        registry.write("ignored.txt", "not a manifest");

        let catalog = load_catalog(registry.path()).expect("catalog should load");

        assert_eq!(catalog.manifest_count, 2);
        assert_eq!(catalog.manifests[0].id, "alpha-tool");
        assert_eq!(catalog.manifests[0].display_name, "Alpha Tool");
        assert_eq!(catalog.manifests[0].kind, "script");
        assert_eq!(catalog.manifests[0].owner, "tom");
        assert_eq!(catalog.manifests[0].project, "toolfoundry");
        assert_eq!(catalog.manifests[0].criticality, "medium");
        assert_eq!(catalog.manifests[0].lifecycle_state, "active");
        assert_eq!(catalog.manifests[0].tags, vec!["registry", "tools"]);
        assert_eq!(catalog.manifests[0].path, alpha_path);
        assert_eq!(catalog.manifests[1].id, "beta-tool");
        assert_eq!(catalog.manifests[1].path, beta_path);
    }

    #[test]
    fn manifest_paths_are_file_only_non_recursive_and_deterministic() {
        let registry = TempRegistry::new();
        let nested = registry.path().join("nested");
        fs::create_dir(&nested).expect("nested directory should be created");
        fs::write(
            nested.join("nested.yaml"),
            manifest("nested-tool", "Nested Tool"),
        )
        .expect("nested manifest should be written");

        let second = registry.write("b.yml", &manifest("second-tool", "Second Tool"));
        registry.write("not-yaml.json", "{}");
        let first = registry.write("a.yaml", &manifest("first-tool", "First Tool"));

        let paths = manifest_paths(registry.path()).expect("manifest paths should load");

        assert_eq!(paths, vec![first, second]);
    }

    #[test]
    fn missing_directory_yields_an_empty_catalog_not_an_error() {
        // A not-yet-configured registry (no directory) means "no tools yet", not a
        // failure. This is the fail-soft contract that keeps `toolfoundry
        // workstate-feed` — and therefore Workstate's snapshot — working before any
        // manifests exist. Previously this returned RegistryRead, which made
        // Workstate's `tools` section Failed on a fresh machine.
        let registry = TempRegistry::new();
        let missing = registry.path().join("missing");

        let paths = manifest_paths(&missing).expect("absent registry dir must be empty, not error");
        assert!(paths.is_empty(), "no directory ⇒ no manifests");

        // And the higher-level catalog load is empty too (the feed export path).
        let catalog = load_catalog(&missing).expect("absent registry dir must load as empty");
        assert_eq!(catalog.manifest_count, 0);
    }

    #[test]
    fn a_real_read_error_still_propagates() {
        // The graceful path is ONLY for "absent". A genuine read error — here a
        // FILE where a directory is expected — must still surface as RegistryRead,
        // so a misconfiguration isn't silently swallowed as "no tools".
        let registry = TempRegistry::new();
        let not_a_dir = registry.write("i-am-a-file", "not a directory");

        let error =
            manifest_paths(&not_a_dir).expect_err("a file-where-a-dir-is-expected must still fail");
        match error {
            ToolFoundryError::RegistryRead { path, .. } => assert_eq!(path, not_a_dir),
            other => panic!("expected registry read error, got {other:?}"),
        }
    }

    fn manifest(id: &str, display_name: &str) -> String {
        ManifestFixture::with_paths(
            format!("~/projects/{id}"),
            format!("~/projects/{id}/target/debug/{id}"),
            format!("~/.local/bin/{id}"),
        )
        .with_identity(
            id,
            display_name,
            format!("Test manifest for {display_name}."),
        )
        .with_tags(&["registry", "tools"])
        .with_project("toolfoundry")
        .with_criticality("medium")
        .yaml()
    }
}
