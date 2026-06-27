//! Data-driven column sizing for the human table.
//!
//! Widths are computed from the actual classified inventory, then capped so one
//! unusually long path or owner does not make the table unreadable.

use crate::core::engine::ClassifiedEntry;

/// Column widths measured in Unicode scalar values, matching the rest of the
/// current renderer's UTF-8-safe truncation strategy.
#[derive(Debug, Clone, Copy)]
pub(super) struct ColumnWidths {
    pub(super) path: usize,
    pub(super) lang: usize,
    pub(super) risk: usize,
    pub(super) category: usize,
    pub(super) owner: usize,
    pub(super) size: usize,
    pub(super) desc: usize,
}

/// Measure all printable content and apply table-friendly caps.
pub(super) fn compute_column_widths(entries: &[ClassifiedEntry]) -> ColumnWidths {
    let mut path_width = "PATH".len();
    let lang_width = 6;
    let risk_width = 8;
    let mut category_width = "CATEGORY".len();
    let mut owner_width = "OWNER".len();
    let size_width = 6;
    let mut desc_width = "DESC".len();

    for entry in entries {
        let path = entry.entry.discovered.path.display().to_string();
        path_width = path_width.max(path.chars().count());

        category_width = category_width.max(entry.classification.category.chars().count());
        owner_width = owner_width.max(entry.classification.owner.chars().count());

        if let Some(description) = &entry.entry.description {
            desc_width = desc_width.max(description.chars().count());
        }
    }

    const MAX_PATH: usize = 72;
    const MAX_CATEGORY: usize = 16;
    const MAX_OWNER: usize = 14;
    const MAX_DESC: usize = 52;

    ColumnWidths {
        path: path_width.min(MAX_PATH),
        lang: lang_width,
        risk: risk_width,
        category: category_width.min(MAX_CATEGORY),
        owner: owner_width.min(MAX_OWNER),
        size: size_width,
        desc: desc_width.min(MAX_DESC),
    }
}

/// Build the header row using exactly the same widths as data rows.
pub(super) fn build_header(widths: &ColumnWidths) -> String {
    format!(
        "{:<path_w$} {:<lang_w$} {:<risk_w$} {:<cat_w$} {:<own_w$} {:>size_w$}  DESC",
        "PATH",
        "LANG",
        "RISK",
        "CATEGORY",
        "OWNER",
        "SIZE",
        path_w = widths.path,
        lang_w = widths.lang,
        risk_w = widths.risk,
        cat_w = widths.category,
        own_w = widths.owner,
        size_w = widths.size,
    )
}
