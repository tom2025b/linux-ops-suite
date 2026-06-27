mod model;
mod paths;

use std::{fs, path::Path};

use crate::{
    error::{Result, ToolFoundryError},
    paths::expand_manifest_path,
};

pub use model::{ConfigInitReport, ConfigReport, ToolFoundryConfig};
pub use paths::{ConfigPaths, default_config_paths};

/// Inspect the effective configuration without writing files.
pub fn inspect_config(path: Option<impl AsRef<Path>>) -> Result<ConfigReport> {
    let paths = default_config_paths()?;
    let config_path = path
        .as_ref()
        .map(|value| value.as_ref().to_path_buf())
        .unwrap_or_else(|| paths.config_path.clone());
    let config_exists = config_path.exists();
    let config = if config_exists {
        load_config(&config_path)?
    } else {
        ToolFoundryConfig::default_for(&paths)
    };

    Ok(ConfigReport {
        config_path,
        config_exists,
        data_directory: paths.data_directory,
        manifest_directory: expand_manifest_path(&config.manifest_directory)?,
    })
}

/// Create or replace a ToolFoundry config and ensure the manifest directory exists.
pub fn init_config(
    path: Option<impl AsRef<Path>>,
    manifest_directory: Option<impl AsRef<Path>>,
    force: bool,
) -> Result<ConfigInitReport> {
    let paths = default_config_paths()?;
    let config_path = path
        .as_ref()
        .map(|value| value.as_ref().to_path_buf())
        .unwrap_or_else(|| paths.config_path.clone());
    let config_existed = config_path.exists();

    if config_existed && !force {
        return Err(ToolFoundryError::ConfigExists { path: config_path });
    }

    let manifest_directory = manifest_directory
        .as_ref()
        .map(|value| value.as_ref().display().to_string())
        .unwrap_or_else(|| {
            ToolFoundryConfig::default_for(&paths)
                .manifest_directory
                .to_string()
        });
    let config = ToolFoundryConfig { manifest_directory };
    let expanded_manifest_directory = expand_manifest_path(&config.manifest_directory)?;

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|source| ToolFoundryError::ConfigWrite {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    fs::create_dir_all(&expanded_manifest_directory).map_err(|source| {
        ToolFoundryError::ConfigWrite {
            path: expanded_manifest_directory.clone(),
            source,
        }
    })?;

    let contents =
        serde_yaml::to_string(&config).map_err(|source| ToolFoundryError::ConfigSerialize {
            path: config_path.clone(),
            source,
        })?;
    fs::write(&config_path, contents).map_err(|source| ToolFoundryError::ConfigWrite {
        path: config_path.clone(),
        source,
    })?;

    Ok(ConfigInitReport {
        config_path,
        config_existed,
        data_directory: paths.data_directory,
        manifest_directory: expanded_manifest_directory,
    })
}

/// Load a ToolFoundry YAML config file from disk.
pub fn load_config(path: impl AsRef<Path>) -> Result<ToolFoundryConfig> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path).map_err(|source| ToolFoundryError::ConfigRead {
        path: path.to_path_buf(),
        source,
    })?;

    serde_yaml::from_str(&contents).map_err(|source| ToolFoundryError::ConfigParse {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::error::ToolFoundryError;

    use super::*;

    #[test]
    fn loads_config_manifest_directory() {
        let workspace = TempWorkspace::new();
        let config_path = workspace.path.join("config.yaml");
        fs::write(&config_path, "manifest_directory: ~/tools/manifests\n")
            .expect("config fixture should be written");

        let config = load_config(&config_path).expect("config should load");

        assert_eq!(config.manifest_directory, "~/tools/manifests");
    }

    #[test]
    fn reports_missing_explicit_config_as_defaulted() {
        let workspace = TempWorkspace::new();
        let config_path = workspace.path.join("missing.yaml");

        let report = inspect_config(Some(&config_path)).expect("config report should load");

        assert_eq!(report.config_path, config_path);
        assert!(!report.config_exists);
    }

    #[test]
    fn initializes_config_without_overwriting_existing_file() {
        let workspace = TempWorkspace::new();
        let config_path = workspace.path.join("config").join("toolfoundry.yaml");

        let report = init_config(
            Some(&config_path),
            Some(workspace.path.join("manifests")),
            false,
        )
        .expect("config should initialize");

        assert_eq!(report.config_path, config_path);
        assert!(!report.config_existed);
        assert!(report.manifest_directory.ends_with("manifests"));
        assert!(report.manifest_directory.is_dir());

        let error = init_config(Some(&report.config_path), None::<&Path>, false)
            .expect_err("existing config should be protected");
        assert!(matches!(error, ToolFoundryError::ConfigExists { .. }));
    }

    #[test]
    fn force_initializes_existing_config() {
        let workspace = TempWorkspace::new();
        let config_path = workspace.path.join("config.yaml");
        fs::write(&config_path, "manifest_directory: /old\n")
            .expect("config fixture should be written");
        let manifest_directory = workspace.path.join("new");

        let report = init_config(Some(&config_path), Some(&manifest_directory), true)
            .expect("config should be overwritten");
        let config = load_config(&config_path).expect("config should reload");

        assert!(report.config_existed);
        assert_eq!(
            config.manifest_directory,
            manifest_directory.display().to_string()
        );
        assert!(manifest_directory.is_dir());
    }

    struct TempWorkspace {
        path: PathBuf,
    }

    impl TempWorkspace {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("toolfoundry-config-{nonce}"));
            fs::create_dir_all(&path).expect("temp workspace should be created");

            Self { path }
        }
    }

    impl Drop for TempWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
