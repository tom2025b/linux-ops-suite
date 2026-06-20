//! In-memory representation of a generated project. Backed by a BTreeMap so
//! iteration order is deterministic (alphabetical by path).
use std::collections::BTreeMap;

#[derive(Debug, Default, Clone)]
pub struct FileTree {
    files: BTreeMap<String, String>,
}

impl FileTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: impl Into<String>, contents: impl Into<String>) {
        self.files.insert(path.into(), contents.into());
    }

    pub fn get(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }

    pub fn paths(&self) -> Vec<&str> {
        self.files.keys().map(String::as_str).collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Render a simple sorted file listing for confirm/summary screens.
    pub fn render_tree(&self) -> String {
        let mut out = String::new();
        for path in self.paths() {
            out.push_str("  ");
            out.push_str(path);
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_sorted_regardless_of_insert_order() {
        let mut t = FileTree::new();
        t.insert("src/main.rs", "fn main() {}");
        t.insert("Cargo.toml", "[package]");
        t.insert(".gitignore", "/target");
        assert_eq!(t.paths(), vec![".gitignore", "Cargo.toml", "src/main.rs"]);
    }

    #[test]
    fn get_returns_inserted_contents() {
        let mut t = FileTree::new();
        t.insert("Cargo.toml", "[package]");
        assert_eq!(t.get("Cargo.toml"), Some("[package]"));
        assert_eq!(t.get("missing"), None);
    }

    #[test]
    fn render_tree_lists_files_sorted() {
        let mut t = FileTree::new();
        t.insert("src/main.rs", "");
        t.insert("Cargo.toml", "");
        let out = t.render_tree();
        assert!(out.contains("Cargo.toml"));
        assert!(out.find("Cargo.toml").unwrap() < out.find("src/main.rs").unwrap());
    }
}
