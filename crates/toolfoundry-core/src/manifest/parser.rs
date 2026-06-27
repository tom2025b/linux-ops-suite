use std::{fs, path::Path};

use crate::{
    error::{Result, ToolFoundryError},
    manifest::ToolManifest,
};

/// Load, parse, and validate a YAML tool manifest from disk.
pub fn load_manifest(path: impl AsRef<Path>) -> Result<ToolManifest> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path).map_err(|source| ToolFoundryError::ManifestRead {
        path: path.to_path_buf(),
        source,
    })?;

    let manifest = serde_yaml::from_str::<ToolManifest>(&contents).map_err(|source| {
        ToolFoundryError::ManifestParse {
            path: path.to_path_buf(),
            source,
        }
    })?;

    manifest.validate()?;
    Ok(manifest)
}
