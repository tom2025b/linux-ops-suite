use std::path::Path;

use toolfoundry_core::{
    config::{ConfigInitReport, ConfigReport},
    registry::ManifestCatalog,
    tui::TuiCatalogView,
};

pub fn print_config_init_report(report: &ConfigInitReport) {
    let action = if report.config_existed {
        "overwrote"
    } else {
        "created"
    };

    println!("config {action}: {}", report.config_path.display());
    println!("data directory: {}", report.data_directory.display());
    println!(
        "manifest directory: {}",
        report.manifest_directory.display()
    );
}

pub fn print_config_report(report: &ConfigReport) {
    println!("config path: {}", report.config_path.display());
    println!("config exists: {}", report.config_exists);
    println!("data directory: {}", report.data_directory.display());
    println!(
        "manifest directory: {}",
        report.manifest_directory.display()
    );
}

pub fn print_catalog(directory: &Path, catalog: &ManifestCatalog) {
    println!(
        "catalog: {} ({} manifests)",
        directory.display(),
        catalog.manifest_count
    );

    for manifest in &catalog.manifests {
        println!(
            "{} | {} | {} | {} | owner={} | criticality={} | review_after={}",
            manifest.id,
            manifest.display_name,
            manifest.kind,
            manifest.lifecycle_state,
            manifest.owner,
            manifest.criticality,
            manifest.review_after
        );
    }
}

pub fn print_tui_catalog_view(directory: &Path, view: &TuiCatalogView) {
    println!(
        "tui catalog: {} ({} tools, {} attention)",
        directory.display(),
        view.tool_count,
        view.attention_count
    );

    let state_counts = view
        .state_counts
        .iter()
        .map(|state| format!("{}={}", state.state, state.count))
        .collect::<Vec<_>>()
        .join(", ");
    println!("states: {state_counts}");
    println!("id | state | owner | criticality | review_after | tags");

    for row in &view.rows {
        println!(
            "{} | {} | {} | {} | {} | {}",
            row.id, row.lifecycle_state, row.owner, row.criticality, row.review_after, row.tags
        );
    }
}
