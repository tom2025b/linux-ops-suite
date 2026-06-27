use std::{env, path::PathBuf};

use serde::Serialize;

use crate::error::{Result, ToolFoundryError};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Default ToolFoundry paths resolved from XDG environment variables.
pub struct ConfigPaths {
    pub config_path: PathBuf,
    pub data_directory: PathBuf,
}

/// Resolve default config and data paths using XDG conventions.
pub fn default_config_paths() -> Result<ConfigPaths> {
    Ok(ConfigPaths {
        config_path: xdg_config_home()?.join("toolfoundry").join("config.yaml"),
        data_directory: xdg_data_home()?.join("toolfoundry"),
    })
}

fn xdg_config_home() -> Result<PathBuf> {
    match env::var_os("XDG_CONFIG_HOME") {
        Some(value) => Ok(PathBuf::from(value)),
        None => Ok(home_dir("XDG_CONFIG_HOME")?.join(".config")),
    }
}

fn xdg_data_home() -> Result<PathBuf> {
    match env::var_os("XDG_DATA_HOME") {
        Some(value) => Ok(PathBuf::from(value)),
        None => Ok(home_dir("XDG_DATA_HOME")?.join(".local").join("share")),
    }
}

fn home_dir(context: &str) -> Result<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| ToolFoundryError::PathExpansion {
            path: context.to_string(),
            reason: "HOME is not set".to_string(),
        })
}
