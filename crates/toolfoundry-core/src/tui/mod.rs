use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::Serialize;

use crate::registry::{ManifestCatalog, ManifestSummary};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Compact catalog view model for terminal dashboard rendering.
pub struct TuiCatalogView {
    pub tool_count: usize,
    pub attention_count: usize,
    pub state_counts: Vec<TuiStateCount>,
    pub rows: Vec<TuiCatalogRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// Count of catalog entries in one lifecycle state.
pub struct TuiStateCount {
    pub state: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
/// One row in the terminal catalog dashboard.
pub struct TuiCatalogRow {
    pub id: String,
    pub display_name: String,
    pub kind: String,
    pub owner: String,
    pub criticality: String,
    pub lifecycle_state: String,
    pub review_after: NaiveDate,
    pub tags: String,
}

/// Build a deterministic terminal view model from a manifest catalog.
pub fn build_catalog_view(catalog: &ManifestCatalog) -> TuiCatalogView {
    let mut states = BTreeMap::<String, usize>::new();
    let mut attention_count = 0;
    let mut rows = Vec::with_capacity(catalog.manifests.len());

    for manifest in &catalog.manifests {
        *states.entry(manifest.lifecycle_state.clone()).or_default() += 1;
        if manifest.lifecycle_state != "active" {
            attention_count += 1;
        }
        rows.push(row_from_manifest(manifest));
    }

    TuiCatalogView {
        tool_count: catalog.manifest_count,
        attention_count,
        state_counts: states
            .into_iter()
            .map(|(state, count)| TuiStateCount { state, count })
            .collect(),
        rows,
    }
}

fn row_from_manifest(manifest: &ManifestSummary) -> TuiCatalogRow {
    TuiCatalogRow {
        id: manifest.id.clone(),
        display_name: manifest.display_name.clone(),
        kind: manifest.kind.clone(),
        owner: manifest.owner.clone(),
        criticality: manifest.criticality.clone(),
        lifecycle_state: manifest.lifecycle_state.clone(),
        review_after: manifest.review_after,
        tags: manifest.tags.join(","),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::NaiveDate;

    use crate::{
        registry::{ManifestCatalog, ManifestSummary},
        tui::build_catalog_view,
    };

    #[test]
    fn builds_catalog_view_with_state_counts() {
        let catalog = ManifestCatalog::new(vec![
            summary("backup-home", "active"),
            summary("cleanup-cache", "stale"),
            summary("rotate-logs", "active"),
        ]);

        let view = build_catalog_view(&catalog);

        assert_eq!(view.tool_count, 3);
        assert_eq!(view.attention_count, 1);
        assert_eq!(view.state_counts[0].state, "active");
        assert_eq!(view.state_counts[0].count, 2);
        assert_eq!(view.state_counts[1].state, "stale");
        assert_eq!(view.rows[0].id, "backup-home");
    }

    fn summary(id: &str, lifecycle_state: &str) -> ManifestSummary {
        ManifestSummary {
            id: id.to_string(),
            display_name: id.replace('-', " "),
            kind: "script".to_string(),
            owner: "tom".to_string(),
            project: "toolfoundry".to_string(),
            criticality: "low".to_string(),
            lifecycle_state: lifecycle_state.to_string(),
            review_after: NaiveDate::from_ymd_opt(2026, 9, 1).expect("date should be valid"),
            tags: vec!["ops".to_string(), "personal".to_string()],
            path: PathBuf::from(format!("{id}.yaml")),
        }
    }
}
