use super::*;
use crate::core::config::Config;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use tempfile::tempdir;

fn make_file(path: &Path, content: &[u8], executable: bool) {
    fs::write(path, content).unwrap();
    if executable {
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }
}

#[test]
fn scan_finds_files_and_respects_depth() {
    let root = tempdir().unwrap();
    let root_path = root.path();

    fs::create_dir_all(root_path.join("bin/sub")).unwrap();
    fs::create_dir_all(root_path.join(".git")).unwrap();

    make_file(&root_path.join("bin/tool1"), b"#!/bin/bash\necho hi", true);
    make_file(&root_path.join("bin/sub/tool2"), b"echo nested", false);
    make_file(&root_path.join("normal.txt"), b"data", false);
    make_file(&root_path.join(".git/ignored"), b"bad", false);

    let yaml = format!(
        r#"
version: 1
scan:
  paths:
    - "{}"
  max_depth: 3
ignore:
  names:
    - ".git"
"#,
        root_path.display()
    );

    let config = Config::from_yaml(&yaml).unwrap();
    let results = scan(&config).unwrap().files;

    let paths: Vec<_> = results
        .iter()
        .map(|file| file.path.file_name().unwrap().to_str().unwrap())
        .collect();

    assert!(paths.contains(&"tool1"));
    assert!(paths.contains(&"tool2"));
    assert!(paths.contains(&"normal.txt"));
    assert!(!paths.contains(&"ignored"));
    assert!(
        results
            .iter()
            .any(|file| file.is_executable && file.path.ends_with("tool1"))
    );
}

#[test]
fn scan_is_deterministic_and_sorted() {
    let root = tempdir().unwrap();
    let root_path = root.path();

    fs::create_dir_all(root_path.join("a")).unwrap();
    fs::create_dir_all(root_path.join("b")).unwrap();

    make_file(&root_path.join("b/z.txt"), b"z", false);
    make_file(&root_path.join("a/mid.txt"), b"m", false);
    make_file(&root_path.join("a/aaa.txt"), b"a", false);

    let yaml = format!(
        r#"
version: 1
scan:
  paths:
    - "{}"
"#,
        root_path.display()
    );
    let config = Config::from_yaml(&yaml).unwrap();

    let first = scan(&config).unwrap().files;
    let second = scan(&config).unwrap().files;

    let first_paths: Vec<_> = first.iter().map(|file| file.path.clone()).collect();
    let second_paths: Vec<_> = second.iter().map(|file| file.path.clone()).collect();

    assert_eq!(first_paths, second_paths);
    let mut sorted = first_paths.clone();
    sorted.sort();
    assert_eq!(first_paths, sorted);
}

#[test]
fn scan_skips_nonexistent_root_gracefully() {
    let yaml = r#"
        version: 1
        scan:
          paths:
            - "/this/does/not/exist/for/sure/123456789"
    "#;
    let config = Config::from_yaml(yaml).unwrap();
    let outcome = scan(&config).unwrap();
    assert!(outcome.files.is_empty());
    // A configured root that simply doesn't exist is normal, not a warning.
    assert!(outcome.warnings.is_empty());
}

#[test]
fn scan_collects_warning_for_unreadable_directory() {
    let root = tempdir().unwrap();
    let root_path = root.path();

    // A readable file at the top so the scan still yields real results.
    make_file(&root_path.join("good.sh"), b"echo good", true);

    // An unreadable subdirectory (mode 0o000). walkdir will fail to read its
    // contents and yield an Err entry — which we want surfaced as a warning
    // rather than silently dropped.
    let locked = root_path.join("locked");
    fs::create_dir(&locked).unwrap();
    make_file(&locked.join("hidden"), b"secret", false);
    let mut perms = fs::metadata(&locked).unwrap().permissions();
    perms.set_mode(0o000);
    fs::set_permissions(&locked, perms).unwrap();

    let yaml = format!(
        r#"
version: 1
scan:
  paths: ["{}"]
"#,
        root_path.display()
    );
    let config = Config::from_yaml(&yaml).unwrap();
    let outcome = scan(&config).unwrap();

    // The readable file is still found (scan does not abort).
    assert!(
        outcome.files.iter().any(|f| f.path.ends_with("good.sh")),
        "readable files must still be discovered"
    );

    // At least one warning was collected for the unreadable directory, and it
    // references the offending path.
    assert!(
        !outcome.warnings.is_empty(),
        "an unreadable directory should produce at least one warning"
    );
    assert!(
        outcome
            .warnings
            .iter()
            .any(|w| w.path.as_deref().is_some_and(|p| p.ends_with("locked"))),
        "the warning should reference the unreadable 'locked' directory"
    );

    // Restore permissions so tempdir cleanup can remove the directory.
    let mut perms = fs::metadata(&locked).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&locked, perms).unwrap();
}

#[test]
fn scan_warns_and_continues_when_a_root_is_a_regular_file() {
    // Pointing a scan root at a regular file (a common fat-finger of
    // `bulwark scan <file>`) must NOT abort the scan. It is treated like a
    // missing root: a warning is recorded and the other roots still scan.
    let root = tempdir().unwrap();
    let root_path = root.path();

    let real_dir = root_path.join("bin");
    fs::create_dir(&real_dir).unwrap();
    make_file(&real_dir.join("good.sh"), b"echo good", true);

    let file_root = root_path.join("not-a-dir.txt");
    make_file(&file_root, b"i am a file", false);

    let yaml = format!(
        r#"
version: 1
scan:
  paths:
    - "{}"
    - "{}"
"#,
        real_dir.display(),
        file_root.display(),
    );
    let config = Config::from_yaml(&yaml).unwrap();
    let outcome = scan(&config).unwrap();

    // The valid directory's file is still discovered.
    assert!(
        outcome.files.iter().any(|f| f.path.ends_with("good.sh")),
        "the valid root must still scan"
    );
    // The file-root produced a warning naming it.
    assert!(
        outcome.warnings.iter().any(|w| w
            .path
            .as_deref()
            .is_some_and(|p| p.ends_with("not-a-dir.txt"))
            && w.message.contains("not a directory")),
        "a non-directory root should warn, got: {:?}",
        outcome.warnings
    );
}

#[test]
fn scan_respects_ignore_names_on_files_and_dirs() {
    let root = tempdir().unwrap();
    let root_path = root.path();

    fs::create_dir_all(root_path.join("target/debug")).unwrap();
    make_file(&root_path.join("good.sh"), b"echo good", true);
    make_file(&root_path.join("target/debug/bad"), b"bad", false);

    let yaml = format!(
        r#"
        version: 1
        scan:
          paths: ["{}"]
        ignore:
          names: ["target"]
        "#,
        root_path.display()
    );

    let config = Config::from_yaml(&yaml).unwrap();
    let results = scan(&config).unwrap().files;

    let names: Vec<_> = results
        .iter()
        .map(|file| file.path.file_name().unwrap().to_str().unwrap())
        .collect();
    assert_eq!(names, vec!["good.sh"]);
}
