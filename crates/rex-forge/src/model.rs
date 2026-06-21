//! Plain data types for bases, components, and a resolved selection.
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Rust,
    Go,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Inject {
    pub target: String,
    pub anchor: String,
    pub template: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FileSpec {
    pub path: String,
    pub template: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Note {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Component {
    pub name: String,
    pub language: Language,
    pub category: String,
    pub summary: String,
    pub bases: Vec<String>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<String>,
    #[serde(default)]
    pub dependencies: toml::Table,
    #[serde(default, rename = "files")]
    pub files: Vec<FileSpec>,
    #[serde(default, rename = "inject")]
    pub injects: Vec<Inject>,
    #[serde(default, rename = "note")]
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Base {
    pub name: String,
    pub language: Language,
    pub summary: String,
    #[serde(default)]
    pub anchors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Selection {
    pub base: String,
    pub components: Vec<String>,
    pub project_name: String,
    pub license: String,
    pub author: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_parses_with_defaults_for_optional_fields() {
        let toml = r#"
            name = "clap"
            language = "rust"
            category = "CLI & Args"
            summary = "Arg parsing"
            bases = ["rust-bin"]
        "#;
        let c: Component = toml::from_str(toml).unwrap();
        assert_eq!(c.name, "clap");
        assert_eq!(c.language, Language::Rust);
        assert!(c.requires.is_empty());
        assert!(c.conflicts.is_empty());
        assert!(c.injects.is_empty());
    }

    #[test]
    fn component_parses_inject_and_requires() {
        let toml = r#"
            name = "metrics"
            language = "rust"
            category = "Observability"
            summary = "Prometheus"
            bases = ["rust-bin"]
            requires = ["config"]

            [[inject]]
            target = "src/main.rs"
            anchor = "rex:init"
            template = "files/init.rs.j2"
        "#;
        let c: Component = toml::from_str(toml).unwrap();
        assert_eq!(c.requires, vec!["config"]);
        assert_eq!(c.injects.len(), 1);
        assert_eq!(c.injects[0].anchor, "rex:init");
    }
}
