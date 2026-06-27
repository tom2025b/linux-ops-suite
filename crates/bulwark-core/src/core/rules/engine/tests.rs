use super::*;
use crate::core::entry::{Language, ScriptEntry};
use crate::core::rules::{RiskLevel, Rule};
use crate::core::scanner::DiscoveredFile;
use std::path::PathBuf;

fn make_file(name: &str, executable: bool) -> DiscoveredFile {
    DiscoveredFile {
        path: PathBuf::from(format!("/home/user/bin/{name}")),
        size: 1234,
        is_executable: executable,
    }
}

/// Like `make_file`, but takes the full absolute path verbatim. Useful for
/// path-prefix tests where the leading directories are the thing under test.
fn make_test_file(path: &str, executable: bool) -> DiscoveredFile {
    DiscoveredFile {
        path: PathBuf::from(path),
        size: 1234,
        is_executable: executable,
    }
}

#[test]
fn default_engine_classifies_user_scripts_as_low_risk() {
    let engine = RuleEngine::with_defaults();

    let script = make_file("deploy.sh", true);
    let class = engine.classify(&script);

    assert_eq!(class.risk, RiskLevel::Low);
    assert_eq!(class.category, "script");
    assert_eq!(class.owner, "user");
}

#[test]
fn default_engine_flags_destructive_commands_high_risk() {
    let engine = RuleEngine::with_defaults();

    let rm = make_file("rm", true);
    let class = engine.classify(&rm);

    assert_eq!(class.risk, RiskLevel::High);
    assert_eq!(class.category, "destructive");
}

#[test]
fn last_matching_rule_wins_deterministic() {
    let yaml = r#"
- name: "all-executables"
  match:
    executable: true
  classify:
    risk: medium
    category: binary
    owner: user

- name: "shell-scripts"
  match:
    names: ["myscript"]
    executable: true
  classify:
    risk: low
    category: script
    owner: user
"#;

    let engine = RuleEngine::from_yaml(yaml).unwrap();
    let file = make_file("myscript", true);
    let class = engine.classify(&file);

    assert_eq!(class.risk, RiskLevel::Low);
    assert_eq!(class.category, "script");
}

#[test]
fn no_match_returns_safe_default() {
    let engine = RuleEngine::with_defaults();
    let weird = make_file("some-random-data-file.txt", false);

    let class = engine.classify(&weird);
    assert_eq!(class.risk, RiskLevel::Low);
    assert_eq!(class.category, "unknown");
}

#[test]
fn from_yaml_rejects_empty_rules() {
    let err = RuleEngine::from_yaml("[]").unwrap_err();
    assert!(format!("{err}").contains("rule list must not be empty"));
}

#[test]
fn extension_matching_tolerates_leading_dot() {
    let dotless = r#"
- name: "py-dotless"
  match:
    extensions: ["py"]
  classify:
    risk: low
    category: script-dotless
    owner: user
"#;
    let dotted = r#"
- name: "py-dotted"
  match:
    extensions: [".py"]
  classify:
    risk: low
    category: script-dotted
    owner: user
"#;

    let mut file = make_file("tool.py", true);
    file.path = PathBuf::from("/home/user/bin/tool.py");

    let dotless_engine = RuleEngine::from_yaml(dotless).unwrap();
    assert_eq!(dotless_engine.classify(&file).category, "script-dotless");

    let dotted_engine = RuleEngine::from_yaml(dotted).unwrap();
    assert_eq!(dotted_engine.classify(&file).category, "script-dotted");

    let mut other = make_file("notes.txt", false);
    other.path = PathBuf::from("/home/user/bin/notes.txt");
    assert_eq!(dotless_engine.classify(&other).category, "unknown");
}

#[test]
fn path_prefix_matching_works() {
    let yaml = r#"
- name: "system-tools"
  match:
    path_prefixes: ["/usr/bin", "/usr/local"]
  classify:
    risk: high
    category: system
    owner: system
"#;

    let engine = RuleEngine::from_yaml(yaml).unwrap();

    let mut file = make_file("ls", true);
    file.path = PathBuf::from("/usr/bin/ls");

    let class = engine.classify(&file);
    assert_eq!(class.risk, RiskLevel::High);
    assert_eq!(class.category, "system");
}

#[test]
fn language_matching_works_with_script_entry() {
    let yaml = r#"
- name: "bash-tools"
  match:
    languages: ["bash"]
  classify:
    risk: low
    category: shell-script
    owner: user
"#;
    let engine = RuleEngine::from_yaml(yaml).unwrap();

    let entry = ScriptEntry {
        discovered: make_file("deploy", true),
        language: Language::Bash,
        description: None,
        sidecar: None,
        sidecar_warning: None,
    };

    let class = engine.classify_entry(&entry);
    assert_eq!(class.category, "shell-script");

    let python = ScriptEntry {
        discovered: make_file("tool", true),
        language: Language::Python,
        description: None,
        sidecar: None,
        sidecar_warning: None,
    };
    assert_eq!(engine.classify_entry(&python).category, "unknown");
}

#[test]
fn language_rule_for_unknown_token_never_matches_undetected_files() {
    // `Unknown` is the absence of a detected language, not a targetable one. A
    // rule that lists "unknown" must NOT capture files Bulwark couldn't classify
    // — otherwise one rule silently swallows everything unrecognized. The file
    // falls through to the safe default ("unknown" category) instead of the rule.
    let yaml = r#"
- name: "catch-unknown"
  match:
    languages: ["unknown"]
  classify:
    risk: critical
    category: should-not-apply
    owner: x
"#;
    let engine = RuleEngine::from_yaml(yaml).unwrap();

    let undetected = ScriptEntry {
        discovered: make_file("mystery-blob", false),
        language: Language::Unknown,
        description: None,
        sidecar: None,
        sidecar_warning: None,
    };

    let class = engine.classify_entry(&undetected);
    assert_eq!(
        class.category, "unknown",
        "languages: [unknown] must not match an undetected file"
    );
    assert_eq!(class.risk, RiskLevel::Low);
}

#[test]
fn user_rules_merge_on_top_of_defaults() {
    let user_yaml = r#"
- name: "override-binary"
  match:
    names: ["ls"]
    executable: true
  classify:
    risk: low
    category: user-tool
    owner: user
"#;

    let additional: Vec<Rule> = serde_yaml::from_str(user_yaml).unwrap();
    let engine = RuleEngine::from_defaults_and_additional(additional);

    let mut ls = make_file("ls", true);
    ls.path = PathBuf::from("/usr/bin/ls");

    let class = engine.classify(&ls);
    assert_eq!(class.risk, RiskLevel::Low);
    assert_eq!(class.category, "user-tool");
}

#[test]
fn path_prefix_respects_component_boundaries() {
    let yaml = r#"
- name: "usr-bin-tools"
  match:
    path_prefixes: ["/usr/bin"]
  classify:
    risk: high
    category: system
    owner: system
"#;
    let engine = RuleEngine::from_yaml(yaml).unwrap();

    // Should match the directory itself
    let dir = make_test_file("/usr/bin", true);
    assert_eq!(engine.classify(&dir).category, "system");

    // Should match children
    let child = make_test_file("/usr/bin/ls", true);
    assert_eq!(engine.classify(&child).category, "system");

    // Trailing slash in the rule should be tolerated
    let engine_slash = RuleEngine::from_yaml(
        r#"- name: s
  match:
    path_prefixes: ["/usr/bin/"]
  classify:
    risk: high
    category: system-slash
    owner: system
"#,
    )
    .unwrap();
    assert_eq!(engine_slash.classify(&child).category, "system-slash");

    // Should NOT match sibling directories
    let sibling = make_test_file("/usr/binary-tools/foo", true);
    assert_eq!(engine.classify(&sibling).category, "unknown");

    let sibling2 = make_test_file("/usr/bin-custom/x", true);
    assert_eq!(engine.classify(&sibling2).category, "unknown");
}
