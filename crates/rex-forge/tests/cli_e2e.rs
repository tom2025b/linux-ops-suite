use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rex-forge"))
}

#[test]
fn non_interactive_generate_creates_compiling_project() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("myapp");
    let out = bin()
        .args([
            "new",
            dest.to_str().unwrap(),
            "--base",
            "rust-bin",
            "--with",
            "clap",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dest.join("Cargo.toml").exists());
    assert!(dest.join("src/cli.rs").exists());

    // The generated project compiles.
    let build = Command::new("cargo")
        .args([
            "build",
            "--manifest-path",
            dest.join("Cargo.toml").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "generated project failed to build: {}",
        String::from_utf8_lossy(&build.stderr)
    );
}

#[test]
fn list_prints_bases_and_components() {
    let out = bin().arg("list").output().unwrap();
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("rust-bin"));
    assert!(s.contains("clap"));
}
