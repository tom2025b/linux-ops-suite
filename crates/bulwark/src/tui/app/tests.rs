use super::*;
use crate::app::ClassifiedEntry;
use crate::core::entry::{Language, ScriptEntry};
use crate::core::rules::{Classification, RiskLevel};
use crate::core::scanner::DiscoveredFile;
use std::path::PathBuf;

fn make_entry(path: &str, desc: Option<&str>, risk: RiskLevel) -> ClassifiedEntry {
    ClassifiedEntry {
        entry: ScriptEntry {
            discovered: DiscoveredFile {
                path: PathBuf::from(path),
                size: 123,
                is_executable: false,
            },
            language: Language::Bash,
            description: desc.map(|s| s.to_string()),
            sidecar: None,
            sidecar_warning: None,
        },
        classification: Classification {
            risk,
            category: "script".into(),
            owner: "user".into(),
        },
    }
}

#[test]
fn filter_matches_path_and_description() {
    let entries = vec![
        make_entry(
            "/home/you/bin/backup.sh",
            Some("Backs up the NAS"),
            RiskLevel::Low,
        ),
        make_entry(
            "/home/you/bin/reconcile.py",
            Some("Monthly invoices"),
            RiskLevel::Low,
        ),
        make_entry("/tmp/weird-tool", None, RiskLevel::Low),
    ];
    let mut app = TuiApp::new(entries, Vec::new(), Vec::new());

    assert_eq!(app.filtered.len(), 3);

    app.filter = "backup".to_string();
    app.rebuild_filtered();
    assert_eq!(app.filtered.len(), 1);
    assert!(
        app.entries[app.filtered[0]]
            .entry
            .discovered
            .path
            .ends_with("backup.sh")
    );

    app.filter = "invoice".to_string();
    app.rebuild_filtered();
    assert_eq!(app.filtered.len(), 1);

    app.filter = "nonexistent".to_string();
    app.rebuild_filtered();
    assert!(app.filtered.is_empty());
}

#[test]
fn selection_clamps_on_filter_change() {
    let entries: Vec<_> = (0..5)
        .map(|i| make_entry(&format!("/bin/tool{i}.sh"), None, RiskLevel::Low))
        .collect();
    let mut app = TuiApp::new(entries, Vec::new(), Vec::new());
    app.selected = 4;

    app.filter = "tool3".to_string();
    app.rebuild_filtered();
    assert_eq!(app.filtered.len(), 1);
    assert_eq!(app.selected, 0);
}

#[test]
fn status_message_persists_until_dismissed() {
    let mut app = TuiApp::new(
        vec![make_entry("/bin/a.sh", None, RiskLevel::Low)],
        Vec::new(),
        Vec::new(),
    );

    app.status_message = Some("rescanned — 1 items (was 1)".to_string());
    // The message must survive until an explicit dismissal (the event loop
    // calls dismiss_status on the next key press, never per-iteration).
    assert!(app.status_message.is_some());

    app.dismiss_status();
    assert!(app.status_message.is_none());
}

fn make_config(paths: &[&str]) -> crate::Config {
    crate::Config {
        version: 1,
        scan: crate::core::config::ScanConfig {
            paths: paths.iter().map(|s| s.to_string()).collect(),
            max_depth: 6,
            follow_symlinks: false,
        },
        ignore: Default::default(),
    }
}

#[test]
fn rescan_preserves_cli_path_overrides() {
    // `bulwark tui /cli/paths` — the rescan reloads the config from disk and
    // must re-apply the CLI overrides, not revert to the config-file paths.
    let overrides = vec!["/cli/paths".to_string()];
    let app = TuiApp::new(Vec::new(), Vec::new(), overrides.clone());

    let mut config = make_config(&["/from/config"]);
    app.apply_path_overrides(&mut config);
    assert_eq!(config.scan.paths, overrides);
}

#[test]
fn rescan_keeps_config_paths_without_cli_overrides() {
    // Plain `bulwark tui` — no overrides, so the config-file paths stand.
    let app = TuiApp::new(Vec::new(), Vec::new(), Vec::new());

    let mut config = make_config(&["/from/config"]);
    app.apply_path_overrides(&mut config);
    assert_eq!(config.scan.paths, vec!["/from/config".to_string()]);
}

#[test]
fn risk_filter_combines_with_text_and_clears() {
    let entries = vec![
        make_entry("/bin/a.sh", Some("foo"), RiskLevel::Low),
        make_entry("/bin/b.sh", Some("foo"), RiskLevel::High),
        make_entry("/bin/c.sh", Some("bar"), RiskLevel::Low),
    ];
    let mut app = TuiApp::new(entries, Vec::new(), Vec::new());
    assert_eq!(app.filtered.len(), 3);

    app.set_risk_filter(Some(RiskLevel::Low));
    assert_eq!(app.filtered.len(), 2);

    app.filter = "foo".to_string();
    app.rebuild_filtered();
    assert_eq!(app.filtered.len(), 1);

    app.set_risk_filter(None);
    app.rebuild_filtered();
    assert_eq!(app.filtered.len(), 2);
    assert_eq!(app.risk_filter, None);
}
