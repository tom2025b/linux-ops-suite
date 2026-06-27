use std::path::Path;

/// ProjectType represents a recognized programming language ecosystem.
/// Each variant has:
/// - A detection strategy (which files to look for)
/// - A set of built-in check profiles (defined in core/checks.rs)
/// - Extensibility: adding a new language means adding a variant here,
///   a detection function below, and profiles in checks.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectType {
    /// Rust project: detected by Cargo.toml and/or Cargo.lock
    Rust,
    /// Go project: detected by go.mod or go.work
    Go,
    /// Node.js project: detected by package.json (future)
    Node,
    /// Python project: detected by pyproject.toml or setup.py (future)
    Python,
}

impl std::fmt::Display for ProjectType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rust => write!(f, "Rust"),
            Self::Go => write!(f, "Go"),
            Self::Node => write!(f, "Node"),
            Self::Python => write!(f, "Python"),
        }
    }
}

/// detect_project_type probes a directory for known project markers and returns
/// the detected project type, or None if no recognized project is found.
///
/// Detection order matters: we check more specific markers (Cargo.toml, which is
/// unique to Rust) before more general ones (package.json, which could appear in
/// other contexts). This prevents false positives.
pub fn detect_project_type(dir: &Path) -> Option<ProjectType> {
    // Check for Rust: presence of Cargo.toml indicates a Rust project.
    // Many Rust projects have both Cargo.toml and Cargo.lock; some (libraries)
    // may have only Cargo.toml. Either is sufficient for detection.
    // Cargo.toml is highly specific to Rust, so we check it first.
    if dir.join("Cargo.toml").exists() {
        return Some(ProjectType::Rust);
    }

    // Check for Go: go.mod marks a module root and go.work marks a workspace.
    // go.sum alone is dependency state, so it is not sufficient for detection.
    if dir.join("go.mod").exists() || dir.join("go.work").exists() {
        return Some(ProjectType::Go);
    }

    // Check for Node: presence of package.json indicates a Node.js project.
    // This is less specific than Cargo.toml (package.json can appear in other
    // contexts), so we check it after Rust and Go.
    if dir.join("package.json").exists() {
        return Some(ProjectType::Node);
    }

    // Check for Python: presence of pyproject.toml or setup.py indicates a Python project.
    // pyproject.toml (PEP 517/518) is the modern standard, but setup.py is common
    // in older projects. Either indicates a Python project.
    if dir.join("pyproject.toml").exists() || dir.join("setup.py").exists() {
        return Some(ProjectType::Python);
    }

    // No recognized project type found. The caller (autocheck command) will
    // fall back to the protocol picker.
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn detects_rust_with_cargo_toml() {
        // Create a temporary directory with a Cargo.toml file.
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("Cargo.toml"), "").unwrap();

        // Should detect Rust.
        assert_eq!(detect_project_type(temp.path()), Some(ProjectType::Rust));
    }

    #[test]
    fn detects_rust_with_cargo_lock() {
        // Cargo.lock alone is less common (requires Cargo.toml), but test it anyway.
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("Cargo.lock"), "").unwrap();

        // Without Cargo.toml, this should NOT detect Rust.
        // (Our detection strategy requires Cargo.toml.)
        assert_eq!(detect_project_type(temp.path()), None);
    }

    #[test]
    fn detects_go_with_go_mod() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("go.mod"), "").unwrap();

        assert_eq!(detect_project_type(temp.path()), Some(ProjectType::Go));
    }

    #[test]
    fn detects_go_with_go_mod_and_go_sum() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("go.mod"), "").unwrap();
        fs::write(temp.path().join("go.sum"), "").unwrap();

        assert_eq!(detect_project_type(temp.path()), Some(ProjectType::Go));
    }

    #[test]
    fn detects_go_with_go_work() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("go.work"), "").unwrap();

        assert_eq!(detect_project_type(temp.path()), Some(ProjectType::Go));
    }

    #[test]
    fn does_not_detect_go_from_go_sum_alone() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("go.sum"), "").unwrap();

        assert_eq!(detect_project_type(temp.path()), None);
    }

    #[test]
    fn detects_node_with_package_json() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("package.json"), "").unwrap();

        assert_eq!(detect_project_type(temp.path()), Some(ProjectType::Node));
    }

    #[test]
    fn detects_python_with_pyproject_toml() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("pyproject.toml"), "").unwrap();

        assert_eq!(detect_project_type(temp.path()), Some(ProjectType::Python));
    }

    #[test]
    fn detects_python_with_setup_py() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("setup.py"), "").unwrap();

        assert_eq!(detect_project_type(temp.path()), Some(ProjectType::Python));
    }

    #[test]
    fn returns_none_for_unrecognized_project() {
        // Empty directory: no markers.
        let temp = TempDir::new().unwrap();
        assert_eq!(detect_project_type(temp.path()), None);
    }

    #[test]
    fn prefers_rust_over_node_if_both_present() {
        // Edge case: monorepo with both Rust and Node projects.
        // Our detection order (Rust first) should win.
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("Cargo.toml"), "").unwrap();
        fs::write(temp.path().join("package.json"), "").unwrap();

        assert_eq!(detect_project_type(temp.path()), Some(ProjectType::Rust));
    }

    #[test]
    fn prefers_rust_over_go_if_both_present() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("Cargo.toml"), "").unwrap();
        fs::write(temp.path().join("go.mod"), "").unwrap();

        assert_eq!(detect_project_type(temp.path()), Some(ProjectType::Rust));
    }

    #[test]
    fn prefers_go_over_node_if_both_present() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("go.mod"), "").unwrap();
        fs::write(temp.path().join("package.json"), "").unwrap();

        assert_eq!(detect_project_type(temp.path()), Some(ProjectType::Go));
    }

    #[test]
    fn prefers_rust_over_python_if_both_present() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("Cargo.toml"), "").unwrap();
        fs::write(temp.path().join("pyproject.toml"), "").unwrap();

        assert_eq!(detect_project_type(temp.path()), Some(ProjectType::Rust));
    }
}
