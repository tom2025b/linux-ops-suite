use rex_forge::error::WriteError;
use rex_forge::filetree::FileTree;
use rex_forge::writer::{write, WriteOpts};

fn tree() -> FileTree {
    let mut t = FileTree::new();
    t.insert("Cargo.toml", "[package]\nname=\"x\"\n");
    t.insert("src/main.rs", "fn main() {}\n");
    t
}

#[test]
fn writes_files_to_empty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("proj");
    write(&tree(), &dest, &WriteOpts { force: false, dry_run: false, git: false }).unwrap();
    assert!(dest.join("Cargo.toml").exists());
    assert!(dest.join("src/main.rs").exists());
}

#[test]
fn refuses_nonempty_dir_without_force() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("proj");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("existing.txt"), "hi").unwrap();
    let err = write(&tree(), &dest, &WriteOpts { force: false, dry_run: false, git: false })
        .unwrap_err();
    assert!(matches!(err, WriteError::TargetNotEmpty(_)));
}

#[test]
fn dry_run_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("proj");
    write(&tree(), &dest, &WriteOpts { force: false, dry_run: true, git: false }).unwrap();
    assert!(!dest.exists());
}

#[test]
fn force_overwrites_nonempty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("proj");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("existing.txt"), "hi").unwrap();
    write(&tree(), &dest, &WriteOpts { force: true, dry_run: false, git: false }).unwrap();
    assert!(dest.join("Cargo.toml").exists());
}
