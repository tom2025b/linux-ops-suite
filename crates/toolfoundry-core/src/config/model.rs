use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::ConfigPaths;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// User configuration loaded from `config.yaml`.
pub struct ToolFoundryConfig {
    pub manifest_directory: String,
}

impl ToolFoundryConfig {
    /// Build the default configuration from resolved XDG paths.
    pub fn default_for(paths: &ConfigPaths) -> Self {
        Self {
            manifest_directory: paths.data_directory.join("manifests").display().to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Resolved configuration state for inspection commands.
pub struct ConfigReport {
    pub config_path: PathBuf,
    pub config_exists: bool,
    pub data_directory: PathBuf,
    pub manifest_directory: PathBuf,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Result of initializing a configuration file and manifest directory.
pub struct ConfigInitReport {
    pub config_path: PathBuf,
    pub config_existed: bool,
    pub data_directory: PathBuf,
    pub manifest_directory: PathBuf,
}
