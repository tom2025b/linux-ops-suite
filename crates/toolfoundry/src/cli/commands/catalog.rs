use std::path::PathBuf;

use toolfoundry_core::{registry::load_catalog, tui::build_catalog_view};

use crate::cli::{
    commands::resolve_manifest_directory,
    output::{print_catalog, print_tui_catalog_view},
};

pub(super) fn report_catalog(
    directory: Option<PathBuf>,
    config: Option<PathBuf>,
    json: bool,
) -> anyhow::Result<()> {
    let directory = resolve_manifest_directory(directory, config)?;
    let catalog = load_catalog(&directory)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&catalog)?);
    } else {
        print_catalog(&directory, &catalog);
    }

    Ok(())
}

pub(super) fn report_tui_catalog(
    directory: Option<PathBuf>,
    config: Option<PathBuf>,
    json: bool,
) -> anyhow::Result<()> {
    let directory = resolve_manifest_directory(directory, config)?;
    let catalog = load_catalog(&directory)?;
    let view = build_catalog_view(&catalog);

    if json {
        println!("{}", serde_json::to_string_pretty(&view)?);
    } else {
        print_tui_catalog_view(&directory, &view);
    }

    Ok(())
}
