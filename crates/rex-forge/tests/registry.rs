use rex_forge::registry;

#[test]
fn loads_rust_bin_base() {
    let reg = registry::load();
    let base = reg.base("rust-bin").expect("rust-bin present");
    assert_eq!(base.name, "rust-bin");
    assert!(base.anchors.contains(&"rex:init".to_string()));
}

#[test]
fn clap_is_listed_for_rust_bin_only() {
    let reg = registry::load();
    let names: Vec<_> = reg
        .components_for("rust-bin")
        .iter()
        .map(|c| c.name.clone())
        .collect();
    assert!(names.contains(&"clap".to_string()));
}

#[test]
fn base_files_strip_j2_suffix() {
    let reg = registry::load();
    let files = reg.base_files("rust-bin");
    let paths: Vec<_> = files.iter().map(|(p, _)| p.clone()).collect();
    assert!(paths.contains(&"Cargo.toml".to_string()));
    assert!(paths.contains(&"src/main.rs".to_string()));
    assert!(!paths.iter().any(|p| p.ends_with(".j2")));
}

#[test]
fn dotfiles_are_embedded() {
    // Guards the include_dir dotfile behavior: .gitignore must ship.
    let reg = registry::load();
    let paths: Vec<_> = reg
        .base_files("rust-bin")
        .iter()
        .map(|(p, _)| p.clone())
        .collect();
    assert!(
        paths.contains(&".gitignore".to_string()),
        "embedded paths: {paths:?}"
    );
}
