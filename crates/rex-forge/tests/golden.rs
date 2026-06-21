use rex_forge::model::Selection;
use rex_forge::{merge, registry, resolve};

fn generate(base: &str, with: &[&str]) -> String {
    let reg = registry::load();
    let sel = Selection {
        base: base.into(),
        components: with.iter().map(|s| (*s).to_string()).collect(),
        project_name: "myapp".into(),
        license: "MIT".into(),
        author: "tomb".into(),
    };
    let plan = resolve::resolve(&reg, &sel.base, &sel.components).unwrap();
    let g = merge::generate(&reg, &plan, &sel).unwrap();
    let mut out = String::new();
    for (path, contents) in g.tree.iter() {
        out.push_str(&format!("=== {path} ===\n{contents}\n"));
    }
    out
}

#[test]
fn snapshot_rust_bin_bare() {
    insta::assert_snapshot!(generate("rust-bin", &[]));
}

#[test]
fn snapshot_rust_bin_with_clap() {
    insta::assert_snapshot!(generate("rust-bin", &["clap"]));
}

#[test]
fn snapshot_rust_bin_full_stack() {
    insta::assert_snapshot!(generate("rust-bin", &["clap", "tracing", "anyhow", "ci-github"]));
}

#[test]
fn snapshot_rust_lib_bare() {
    insta::assert_snapshot!(generate("rust-lib", &[]));
}

#[test]
fn snapshot_go_bin_with_flag_slog() {
    insta::assert_snapshot!(generate("go-bin", &["flag", "slog"]));
}

#[test]
fn snapshot_metrics_pulls_in_config() {
    // metrics requires config -> config must appear even though only metrics asked.
    insta::assert_snapshot!(generate("rust-bin", &["metrics"]));
}
