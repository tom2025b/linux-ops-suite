// Built-in check profiles for supported languages.

use crate::core::detector::ProjectType;

#[derive(Debug, Clone)]
pub struct Check {
    pub id: String,
    pub name: String,
    pub command: String,
}

#[derive(Debug, Clone)]
pub struct CheckProfile {
    pub name: String,
    pub description: String,
    pub checks: Vec<Check>,
}

impl CheckProfile {
    fn new(name: &str, description: &str, checks: Vec<(&str, &str, &str)>) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            checks: checks
                .into_iter()
                .map(|(id, name, cmd)| Check {
                    id: id.to_string(),
                    name: name.to_string(),
                    command: cmd.to_string(),
                })
                .collect(),
        }
    }
}

pub fn rust_profiles() -> Vec<CheckProfile> {
    vec![
        CheckProfile::new(
            "Standard Review",
            "Build, test, format, and lint checks",
            vec![
                ("build", "Build", "cargo build --all-targets"),
                ("test", "Tests", "cargo test --all"),
                ("fmt", "Format check", "cargo fmt --all -- --check"),
                (
                    "clippy",
                    "Clippy",
                    "cargo clippy --all-targets -- -D warnings",
                ),
            ],
        ),
        CheckProfile::new(
            "Full Suite",
            "Comprehensive: build, test, lint, and docs",
            vec![
                ("build", "Build", "cargo build --all-targets"),
                ("test", "Tests", "cargo test --all"),
                ("fmt", "Format check", "cargo fmt --all -- --check"),
                (
                    "clippy",
                    "Clippy",
                    "cargo clippy --all-targets -- -D warnings",
                ),
                ("doc", "Documentation", "cargo doc --no-deps"),
                ("doc-test", "Doc tests", "cargo test --doc"),
            ],
        ),
        CheckProfile::new(
            "Quick Check",
            "Fast: just build and format check",
            vec![
                ("build", "Build", "cargo build --all-targets"),
                ("fmt", "Format check", "cargo fmt --all -- --check"),
            ],
        ),
        CheckProfile::new(
            "Strict Mode",
            "Strict: clippy and security checks",
            vec![
                (
                    "clippy",
                    "Clippy (strict)",
                    "cargo clippy --all-targets -- -D warnings",
                ),
                ("audit", "Security audit", "cargo audit"),
            ],
        ),
    ]
}

pub fn go_profiles() -> Vec<CheckProfile> {
    vec![
        CheckProfile::new(
            "Standard Review",
            "Test, vet, and module consistency checks",
            vec![
                ("test", "Tests", "go test ./..."),
                ("vet", "Vet", "go vet ./..."),
                ("mod-tidy", "Module tidy check", "go mod tidy -diff"),
            ],
        ),
        CheckProfile::new(
            "Quick Check",
            "Fast: run package tests",
            vec![("test", "Tests", "go test ./...")],
        ),
        CheckProfile::new(
            "Full Suite",
            "Comprehensive: tests, race detector, vet, and module check",
            vec![
                ("test", "Tests", "go test ./..."),
                ("race", "Race detector", "go test -race ./..."),
                ("vet", "Vet", "go vet ./..."),
                ("mod-tidy", "Module tidy check", "go mod tidy -diff"),
            ],
        ),
    ]
}

pub fn python_profiles() -> Vec<CheckProfile> {
    vec![]
}

pub fn node_profiles() -> Vec<CheckProfile> {
    vec![]
}

pub fn profiles_for_language(lang: ProjectType) -> Vec<CheckProfile> {
    match lang {
        ProjectType::Rust => rust_profiles(),
        ProjectType::Go => go_profiles(),
        ProjectType::Python => python_profiles(),
        ProjectType::Node => node_profiles(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_has_standard_review_first() {
        let profiles = rust_profiles();
        assert_eq!(profiles[0].name, "Standard Review");
    }

    #[test]
    fn quick_check_has_two_checks() {
        let profiles = rust_profiles();
        let quick = profiles.iter().find(|p| p.name == "Quick Check").unwrap();
        assert_eq!(quick.checks.len(), 2);
    }

    #[test]
    fn full_suite_has_more_checks_than_standard() {
        let profiles = rust_profiles();
        let standard = &profiles[0];
        let full = profiles.iter().find(|p| p.name == "Full Suite").unwrap();
        assert!(full.checks.len() > standard.checks.len());
    }

    #[test]
    fn all_checks_have_non_empty_commands() {
        for profile in rust_profiles().into_iter().chain(go_profiles()) {
            for check in &profile.checks {
                assert!(!check.command.is_empty());
            }
        }
    }

    #[test]
    fn profiles_for_language_rust_works() {
        let profiles = profiles_for_language(ProjectType::Rust);
        assert!(!profiles.is_empty());
    }

    #[test]
    fn go_has_standard_review_first() {
        let profiles = go_profiles();
        assert_eq!(profiles[0].name, "Standard Review");
    }

    #[test]
    fn go_standard_review_has_test_vet_and_mod_tidy() {
        let profiles = go_profiles();
        let standard = &profiles[0];
        let ids: Vec<&str> = standard
            .checks
            .iter()
            .map(|check| check.id.as_str())
            .collect();

        assert_eq!(ids, vec!["test", "vet", "mod-tidy"]);
    }

    #[test]
    fn profiles_for_language_go_works() {
        let profiles = profiles_for_language(ProjectType::Go);
        assert!(!profiles.is_empty());
    }
}
