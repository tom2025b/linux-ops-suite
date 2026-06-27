use std::path::PathBuf;

use bulwark::{
    BulwarkError, Classification, ClassifiedEntry, ClassifiedInventory, ColorChoice, Config,
    DiscoveredFile, Inventory, Language, MatchSpec, RiskLevel, Rule, RuleEngine, ScanWarning,
    ScriptEntry, app, core, model, render_human_table, render_json_classified,
    render_markdown_table_classified,
};

#[test]
fn root_reexports_preserve_downstream_import_paths() {
    let config = Config::from_yaml(
        r#"
        version: 1
        scan:
          paths:
            - "/tmp"
          max_depth: 1
        "#,
    )
    .unwrap();
    assert_eq!(config.scan.max_depth, 1);

    let discovered = DiscoveredFile {
        path: PathBuf::from("/tmp/tool.sh"),
        size: 42,
        is_executable: true,
    };
    let entry = ScriptEntry {
        discovered,
        language: Language::Bash,
        description: Some("local tool".to_string()),
        sidecar: None,
        sidecar_warning: None,
    };
    let inventory = Inventory {
        entries: vec![entry.clone()],
        warnings: vec![ScanWarning {
            path: Some(PathBuf::from("/tmp/locked")),
            message: "permission denied".to_string(),
        }],
    };
    let app_inventory = app::Inventory {
        entries: inventory.entries.clone(),
        warnings: inventory.warnings.clone(),
    };
    assert_eq!(app_inventory.entries.len(), 1);
    assert_eq!(app_inventory.warnings.len(), 1);

    let classification = Classification {
        risk: RiskLevel::Low,
        category: "script".to_string(),
        owner: "user".to_string(),
    };
    let classified = ClassifiedEntry {
        entry,
        classification: classification.clone(),
    };

    // ClassifiedInventory is the public return shape of collect_classified_inventory.
    let classified_inventory = ClassifiedInventory {
        entries: vec![classified.clone()],
        warnings: Vec::new(),
    };
    assert_eq!(classified_inventory.entries.len(), 1);
    assert!(classified_inventory.warnings.is_empty());
    let _: ClassifiedInventory = app::ClassifiedInventory::default();

    let entries = [classified];
    assert!(
        render_json_classified(&entries)
            .unwrap()
            .contains("\"risk\": \"low\"")
    );
    assert!(render_markdown_table_classified(&entries).contains("| /tmp/tool.sh | Bash | Low |"));
    assert!(render_human_table(&entries, ColorChoice::Never).contains("Bulwark Scan Results"));

    let rule = Rule {
        name: "compat-rule".to_string(),
        description: None,
        r#match: MatchSpec::default(),
        classify: classification,
    };
    let engine = RuleEngine::with_defaults();
    assert!(engine.rule_count() > 0);
    assert_eq!(rule.r#match, core::rules::MatchSpec::default());
    assert_eq!(model::RiskLevel::Low, RiskLevel::Low);

    let err = BulwarkError::config("compat check");
    assert!(err.to_string().contains("compat check"));
}
