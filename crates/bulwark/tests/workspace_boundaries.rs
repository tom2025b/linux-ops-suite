// Architectural guard-rails for the bulwark crates, adapted when Bulwark was
// consolidated from its standalone repo into the linux-ops-suite umbrella
// workspace. The packaging-shape assertions now describe the umbrella layout
// (workspace inheritance, in-tree path deps, explicit edition 2024 since the
// umbrella root is 2021), but the load-bearing invariant is unchanged:
// bulwark-core must stay free of CLI/TUI/error-framework dependencies.
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("bulwark crate must live under crates/bulwark")
        .to_path_buf()
}

fn read_manifest(relative_path: &str) -> String {
    let path = repo_root().join(relative_path);
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}

#[test]
fn bulwark_crates_registered_in_umbrella_workspace() {
    let manifest = read_manifest("Cargo.toml");

    assert!(manifest.contains("[workspace]"));
    // Both crates must be explicit members of the umbrella workspace.
    assert!(manifest.contains(r#""crates/bulwark""#));
    assert!(manifest.contains(r#""crates/bulwark-core""#));
    assert!(manifest.contains(r#"resolver = "2""#));
    assert!(manifest.contains("[workspace.package]"));
    // bulwark-core is centralized as an in-tree path dependency for consumers.
    assert!(manifest.contains(r#"bulwark-core = { path = "crates/bulwark-core" }"#));
    assert!(
        !manifest.contains("[package]"),
        "workspace root must not become another package"
    );
}

#[test]
fn binary_crate_keeps_public_package_identity() {
    let manifest = read_manifest("crates/bulwark/Cargo.toml");

    assert!(manifest.contains(r#"name = "bulwark""#));
    assert!(manifest.contains("version.workspace = true"));
    // Explicit 2024 because the umbrella's [workspace.package] edition is 2021.
    assert!(manifest.contains(r#"edition = "2024""#));
    assert!(manifest.contains("rust-version.workspace = true"));
    assert!(manifest.contains("bulwark-core = { workspace = true }"));
    assert!(manifest.contains(r#"default = ["tui"]"#));
    // The TUI feature gates the shared suite-ui chrome alongside ratatui/crossterm,
    // so a headless/CLI-only build (without `tui`) pulls in none of them.
    assert!(manifest.contains(r#"tui = ["dep:ratatui", "dep:crossterm", "dep:suite-ui"]"#));
}

#[test]
fn core_crate_stays_free_of_cli_and_tui_dependencies() {
    let manifest = read_manifest("crates/bulwark-core/Cargo.toml");

    assert!(manifest.contains(r#"name = "bulwark-core""#));
    assert!(manifest.contains("version.workspace = true"));
    assert!(manifest.contains(r#"edition = "2024""#));
    assert!(manifest.contains("rust-version.workspace = true"));
    assert!(manifest.contains("thiserror = { workspace = true }"));

    for dependency in ["anyhow", "clap", "ratatui", "crossterm"] {
        assert!(
            !manifest.lines().any(|line| line.starts_with(dependency)),
            "bulwark-core must not depend on {dependency}"
        );
    }
}
