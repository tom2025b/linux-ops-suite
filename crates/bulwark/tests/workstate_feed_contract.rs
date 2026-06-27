use std::fs;
use std::process::Command;

use tempfile::tempdir;

#[test]
fn workstate_feed_stdout_matches_contract_shape() {
    let scan_root = tempdir().unwrap();
    let config_home = tempdir().unwrap();
    let home = tempdir().unwrap();

    let script = scan_root.path().join("tool.sh");
    let content = "#!/bin/bash\n# local tool\necho ok\n";
    fs::write(&script, content).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bulwark"))
        .arg("workstate-feed")
        .arg("--generated-at")
        .arg("2026-06-06T12:34:56Z")
        .arg(scan_root.path())
        .env("XDG_CONFIG_HOME", config_home.path())
        .env("HOME", home.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "expected successful workstate-feed, stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");

    let expected = format!(
        concat!(
            "{{\n",
            "  \"schema_version\": 1,\n",
            "  \"source_tool\": \"bulwark\",\n",
            "  \"generated_at\": \"2026-06-06T12:34:56Z\",\n",
            "  \"item_count\": 1,\n",
            "  \"items\": [\n",
            "    {{\n",
            "      \"id\": \"{}\",\n",
            "      \"name\": \"tool.sh\",\n",
            "      \"severity\": \"low\",\n",
            "      \"path\": \"{}\",\n",
            "      \"language\": \"Bash\",\n",
            "      \"size\": {},\n",
            "      \"is_executable\": false,\n",
            "      \"description\": \"local tool\",\n",
            "      \"risk\": \"low\",\n",
            "      \"category\": \"unknown\",\n",
            "      \"owner\": \"user\"\n",
            "    }}\n",
            "  ]\n",
            "}}\n"
        ),
        script.display(),
        script.display(),
        content.len(),
    );

    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}
