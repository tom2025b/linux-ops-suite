//! Mutable TUI state and pure state transitions.

use anyhow::Result;

use crate::RiskLevel;
use crate::ScanWarning;
use crate::app::ClassifiedEntry;

/// Internal sort modes for the TUI table view (local to this cockpit, does not
/// affect the deterministic path-sorted data coming from the core engine).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum SortMode {
    #[default]
    Path,
    Risk,
    Size,
}

/// The complete mutable state of the Bulwark TUI cockpit.
#[derive(Debug)]
pub struct TuiApp {
    /// The full results from the last successful scan (or rescan).
    /// Never mutated in place; we replace the whole vec on rescan.
    pub entries: Vec<ClassifiedEntry>,

    /// Non-fatal warnings from the last scan (e.g. an unreadable directory).
    /// Surfaced as a count in the status bar so the user knows when the
    /// inventory is partial. Replaced wholesale on rescan.
    pub warnings: Vec<ScanWarning>,

    /// Current filter string (case-insensitive contains match against path + description).
    pub filter: String,

    /// Indices into `entries` that match the current filter (or all if filter empty).
    /// Maintained so the table only ever renders a small Vec and selection is simple.
    pub filtered: Vec<usize>,

    /// Index into the `filtered` vec of the currently highlighted row.
    pub selected: usize,

    /// When true we show the centered help popup instead of (or overlaid on) the main UI.
    pub show_help: bool,

    /// When true (the default), the details pane is visible on the right.
    /// Pressing 'd' toggles this so the results table can use the full width
    /// of the main area when you want to see more rows at once.
    pub show_details: bool,

    /// Optional quick risk filter (set via l/m/h/c keys; None = show all risks).
    /// Combined with the text filter (AND semantics) in rebuild_filtered.
    pub risk_filter: Option<RiskLevel>,

    /// Current sort mode for the table view. Cycles with 's' key.
    /// Does not change the source data (core always provides path-sorted results
    /// for determinism across runs and tools).
    pub sort: SortMode,

    /// When true, printable characters are appended to the filter instead of
    /// being interpreted as commands. Enter or Esc leaves this mode.
    pub filter_mode: bool,

    /// Optional status message shown in the status bar for one "tick"
    /// (e.g. "Rescanned — 142 items", "Filter cleared").
    pub status_message: Option<String>,

    /// The resolved suite-ui palette for this run, honouring `NO_COLOR`. Resolved
    /// once at construction (Bulwark has no `--color`/`--theme` flags, so this is
    /// the `Auto`/cyan default) and read by every renderer, so all styling routes
    /// through the suite's single `NO_COLOR` gate — which the old inline
    /// `Style::default().fg(..)` calls did not have.
    pub theme: suite_ui::Theme,

    /// CLI path overrides from `bulwark tui <paths>` (empty when none given).
    /// Rescan reloads the config from disk, so it must re-apply these to keep
    /// scanning the paths the user asked for instead of silently reverting to
    /// the config-file paths.
    pub path_overrides: Vec<String>,
}

impl TuiApp {
    /// Create a fresh TUI app from a (possibly empty) classified inventory.
    ///
    /// We immediately build the filtered list (initially all items) and
    /// clamp selection to 0.
    pub fn new(
        entries: Vec<ClassifiedEntry>,
        warnings: Vec<ScanWarning>,
        path_overrides: Vec<String>,
    ) -> Self {
        let mut app = Self {
            entries,
            warnings,
            path_overrides,
            filter: String::new(),
            filtered: Vec::new(),
            selected: 0,
            show_help: false,
            show_details: true,
            risk_filter: None,
            sort: SortMode::default(),
            filter_mode: false,
            status_message: None,
            // Auto + cyan default, layered over NO_COLOR — Bulwark exposes no
            // colour/theme flags, so this is the whole palette decision.
            theme: suite_ui::Theme::resolve(
                suite_ui::ColorChoice::Auto,
                suite_ui::ThemeChoice::Cyan,
            ),
        };
        app.rebuild_filtered();
        // If the scan was partial, say so up front so it isn't missed.
        if !app.warnings.is_empty() {
            let msg = format!(
                "⚠ {} scan warning(s) — some paths could not be read",
                app.warnings.len()
            );
            app.set_status(&msg);
        }
        app
    }

    /// The resolved suite-ui palette for this run (honours `NO_COLOR`). Every
    /// renderer reads this so styling goes through one gate.
    pub fn theme(&self) -> suite_ui::Theme {
        self.theme
    }

    /// Recompute `filtered` from the current text filter + risk_filter.
    pub fn rebuild_filtered(&mut self) {
        let q = self.filter.to_lowercase();
        let risk = self.risk_filter;
        self.filtered = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                let text_ok = if q.is_empty() {
                    true
                } else {
                    let path = e.entry.discovered.path.to_string_lossy().to_lowercase();
                    let desc = e.entry.description.as_deref().unwrap_or("").to_lowercase();
                    path.contains(&q) || desc.contains(&q)
                };
                let risk_ok = risk.is_none_or(|r| e.classification.risk == r);
                text_ok && risk_ok
            })
            .map(|(i, _)| i)
            .collect();

        self.apply_sort();

        if self.filtered.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len() - 1;
        }
    }

    /// Move selection down (j / down arrow). Clamps at the bottom.
    pub fn select_next(&mut self) {
        if !self.filtered.is_empty() && self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    /// Move selection up. Clamps at 0.
    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Jump to first item.
    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    /// Jump to last item.
    pub fn select_last(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = self.filtered.len() - 1;
        }
    }

    /// Page up in the table (for long lists of results).
    pub fn select_page_up(&mut self) {
        const PAGE: usize = 15;
        if self.selected >= PAGE {
            self.selected -= PAGE;
        } else {
            self.selected = 0;
        }
    }

    /// Page down in the table.
    pub fn select_page_down(&mut self) {
        const PAGE: usize = 15;
        if !self.filtered.is_empty() {
            let max = self.filtered.len() - 1;
            if self.selected + PAGE <= max {
                self.selected += PAGE;
            } else {
                self.selected = max;
            }
        }
    }

    /// Enter filter mode (user pressed `/`).
    pub fn begin_filter(&mut self) {
        self.filter_mode = true;
        self.show_help = false;
    }

    /// Append a character to the live filter and rebuild.
    pub fn push_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.rebuild_filtered();
    }

    /// Backspace in filter mode.
    pub fn filter_backspace(&mut self) {
        self.filter.pop();
        self.rebuild_filtered();
    }

    /// Commit / leave filter mode (Enter). Filter text stays active.
    pub fn commit_filter(&mut self) {
        self.filter_mode = false;
        if !self.filter.is_empty() {
            self.set_status(&format!("filtered: {}", self.filter));
        }
    }

    /// Clear the filter entirely and leave filter mode.
    pub fn clear_filter(&mut self) {
        let had_filter = !self.filter.is_empty();
        self.filter.clear();
        self.filter_mode = false;
        self.rebuild_filtered();
        if had_filter {
            self.set_status("filter cleared");
        }
    }

    /// Toggle the help overlay.
    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
        if self.show_help {
            self.filter_mode = false;
        }
    }

    /// Toggle the details pane (full-width table vs split view).
    pub fn toggle_details(&mut self) {
        self.show_details = !self.show_details;
        if self.show_details {
            self.set_status("details shown");
        } else {
            self.set_status("details hidden — 'd' to restore");
        }
    }

    /// Set (or clear) the quick risk filter and rebuild.
    pub fn set_risk_filter(&mut self, risk: Option<RiskLevel>) {
        self.risk_filter = risk;
        self.filter_mode = false;
        self.show_help = false;
        self.rebuild_filtered();
        self.selected = 0;
        match risk {
            Some(r) => self.set_status(&format!("risk: {:?}", r)),
            None => self.set_status("risk filter cleared (all levels)"),
        }
    }

    /// Resort the current `filtered` index list according to `self.sort`.
    fn apply_sort(&mut self) {
        let entries = &self.entries;
        match self.sort {
            SortMode::Path => {}
            SortMode::Risk => {
                self.filtered
                    .sort_by_key(|&i| std::cmp::Reverse(entries[i].classification.risk));
            }
            SortMode::Size => {
                self.filtered
                    .sort_by_key(|&i| std::cmp::Reverse(entries[i].entry.discovered.size));
            }
        }
    }

    /// Cycle to the next sort mode and re-apply to the current filtered view.
    pub fn cycle_sort(&mut self) {
        let selected_path = self
            .filtered
            .get(self.selected)
            .map(|&i| self.entries[i].entry.discovered.path.clone());

        self.sort = match self.sort {
            SortMode::Path => SortMode::Risk,
            SortMode::Risk => SortMode::Size,
            SortMode::Size => SortMode::Path,
        };
        self.apply_sort();

        if let Some(path) = selected_path {
            if let Some(new_pos) = self
                .filtered
                .iter()
                .position(|&i| self.entries[i].entry.discovered.path == path)
            {
                self.selected = new_pos;
            } else {
                self.selected = 0;
            }
        } else {
            self.selected = 0;
        }

        let label = match self.sort {
            SortMode::Path => "path (default)",
            SortMode::Risk => "risk (high first)",
            SortMode::Size => "size (largest first)",
        };
        self.set_status(&format!("sorted by {}", label));
    }

    /// Export the currently filtered entries as pretty JSON to a file in the
    /// current directory. Reuses the exact same renderer as `bulwark scan --json`.
    pub fn export_current_view(&mut self) -> Result<()> {
        let filtered_entries: Vec<_> = self
            .filtered
            .iter()
            .map(|&i| self.entries[i].clone())
            .collect();

        let json = crate::render_json_classified(&filtered_entries)
            .map_err(|e| anyhow::anyhow!("failed to render JSON: {e}"))?;

        std::fs::write("bulwark-tui-export.json", json)
            .map_err(|e| anyhow::anyhow!("failed to write export file: {e}"))?;

        self.set_status("exported filtered view to bulwark-tui-export.json");
        Ok(())
    }

    /// Re-apply the CLI path overrides (if any) to a freshly loaded config.
    ///
    /// Pure (no I/O) so tests can prove that rescan keeps the paths from
    /// `bulwark tui <paths>` rather than reverting to the config-file paths.
    pub(crate) fn apply_path_overrides(&self, config: &mut crate::Config) {
        if !self.path_overrides.is_empty() {
            config.scan.paths = self.path_overrides.clone();
        }
    }

    /// Perform a fresh classification using the library (same call main.rs uses).
    pub fn rescan(&mut self) -> Result<()> {
        let mut config =
            crate::Config::load().map_err(|e| anyhow::anyhow!("rescan config load: {e}"))?;
        // Keep honouring `bulwark tui <paths>` — main.rs applied these at
        // startup, and reloading the config must not drop them.
        self.apply_path_overrides(&mut config);

        // Recompute missing-root warnings here, exactly as main.rs does at
        // startup. The scanner treats a missing configured root as a silent skip
        // (it only emits warnings for roots that exist but can't be read), so if
        // we relied on `new_inventory.warnings` alone, the cockpit would flip to
        // a "clean" state after the first rescan even while configured
        // directories are still missing. Prepending these keeps rescan's warning
        // set consistent with the initial launch.
        let mut warnings = config
            .missing_scan_path_warnings()
            .map_err(|e| anyhow::anyhow!("rescan resolve scan paths: {e}"))?;

        let new_inventory = crate::collect_classified_inventory(&config)
            .map_err(|e| anyhow::anyhow!("rescan classification: {e}"))?;

        warnings.extend(new_inventory.warnings);

        let old_count = self.entries.len();
        self.entries = new_inventory.entries;
        self.warnings = warnings;
        self.filter.clear();
        self.risk_filter = None;
        self.filter_mode = false;
        self.show_help = false;
        self.rebuild_filtered();
        self.selected = 0;

        let mut msg = format!(
            "rescanned — {} items (was {})",
            self.entries.len(),
            old_count
        );
        if !self.warnings.is_empty() {
            msg.push_str(&format!(", ⚠ {} warning(s)", self.warnings.len()));
        }
        self.set_status(&msg);
        Ok(())
    }

    fn set_status(&mut self, msg: &str) {
        self.status_message = Some(msg.to_string());
    }

    /// Dismiss the status message. Called on every key press, so a message
    /// stays on screen (through idle redraws) until the user next interacts —
    /// the same semantics as RexOps's footer toast.
    pub(crate) fn dismiss_status(&mut self) {
        self.status_message = None;
    }

    /// Build the canonical "view state" tokens describing the active sort,
    /// text filter, and risk filter — e.g. `["sort:risk", "filter:backup",
    /// "risk:High"]`. Returns an empty vec when the view is in its default
    /// state (path sort, no filters).
    ///
    /// This is the **single source of truth** for that summary. The status bar
    /// and the help popup both render it (wrapped in their own styling), so
    /// extracting it here means the two views can never drift out of sync — a
    /// foot-gun the previous hand-duplicated copies had. The header uses its
    /// own separate colored badges, so it intentionally does not call this.
    pub(crate) fn view_state_tokens(&self) -> Vec<String> {
        let mut tokens = Vec::new();
        if self.sort != SortMode::Path {
            let lbl = match self.sort {
                SortMode::Risk => "risk",
                SortMode::Size => "size",
                SortMode::Path => "path",
            };
            tokens.push(format!("sort:{lbl}"));
        }
        if !self.filter.is_empty() {
            tokens.push(format!("filter:{}", self.filter));
        }
        if let Some(r) = self.risk_filter {
            tokens.push(format!("risk:{r:?}"));
        }
        tokens
    }
}
