use rex_forge::model::Selection;
use rex_forge::writer::{write, WriteOpts};
use rex_forge::{merge, registry, resolve};
use std::process::Command;

fn gen_to(dir: &std::path::Path, base: &str, with: &[&str]) {
    let reg = registry::load();
    let sel = Selection {
        base: base.into(),
        components: with.iter().map(|s| (*s).to_string()).collect(),
        project_name: "gen".into(),
        license: "MIT".into(),
        author: "t".into(),
    };
    let plan = resolve::resolve(&reg, &sel.base, &sel.components).unwrap();
    let g = merge::generate(&reg, &plan, &sel).unwrap();
    write(
        &g.tree,
        dir,
        &WriteOpts {
            force: true,
            dry_run: false,
            git: false,
        },
    )
    .unwrap();
}

#[test]
fn generated_rust_bin_full_stack_compiles() {
    let tmp = tempfile::tempdir().unwrap();
    gen_to(tmp.path(), "rust-bin", &["clap", "tracing", "anyhow"]);
    let out = Command::new("cargo")
        .args([
            "build",
            "--manifest-path",
            tmp.path().join("Cargo.toml").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn generated_rust_lib_compiles() {
    let tmp = tempfile::tempdir().unwrap();
    gen_to(tmp.path(), "rust-lib", &["anyhow"]);
    let out = Command::new("cargo")
        .args([
            "build",
            "--manifest-path",
            tmp.path().join("Cargo.toml").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn generated_go_bin_compiles_when_go_present() {
    let go_present = Command::new("go")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !go_present {
        eprintln!("skipping: go toolchain not installed");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    gen_to(tmp.path(), "go-bin", &["flag", "slog"]);
    let out = Command::new("go")
        .args(["build", "./..."])
        .current_dir(tmp.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
