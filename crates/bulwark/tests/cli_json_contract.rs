use std::fs;
use std::process::Command;

use tempfile::tempdir;

#[test]
fn scan_json_stdout_matches_contract_shape() {
    let scan_root = tempdir().unwrap();
    let config_home = tempdir().unwrap();
    let home = tempdir().unwrap();

    let script = scan_root.path().join("tool.sh");
    let content = "#!/bin/bash\n# local tool\necho ok\n";
    fs::write(&script, content).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bulwark"))
        .arg("scan")
        .arg("--json")
        .arg(scan_root.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("HOME", home.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "expected successful scan, stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");

    let expected = format!(
        concat!(
            "[\n",
            "  {{\n",
            "    \"path\": \"{}\",\n",
            "    \"language\": \"Bash\",\n",
            "    \"size\": {},\n",
            "    \"is_executable\": false,\n",
            "    \"description\": \"local tool\",\n",
            "    \"risk\": \"low\",\n",
            "    \"category\": \"unknown\",\n",
            "    \"owner\": \"user\"\n",
            "  }}\n",
            "]\n"
        ),
        script.display(),
        content.len(),
    );

    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}
