#![cfg(not(feature = "tui"))]

use std::process::Command;

#[test]
fn no_default_features_reports_tui_disabled_for_default_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_bulwark"))
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "default command should fail when TUI support is disabled"
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error: this bulwark binary was built without TUI support"));
    assert!(stderr.contains("Rebuild with default features or --features tui"));
}
