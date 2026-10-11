use assert_cmd::Command;

#[test]
fn detects_mjs_files() {
    let mut cmd = Command::cargo_bin("anc").unwrap();
    cmd.arg("tests/fixtures/node-mjs-test")
        .arg("--output")
        .arg("json");
    
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    // Parse the JSON output
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| {
            eprintln!("Failed to parse JSON: {}", e);
            eprintln!("STDOUT: {}", stdout);
            eprintln!("STDERR: {}", String::from_utf8_lossy(&output.stderr));
            panic!("Failed to parse JSON output");
        });
    
    // Check that we detected a Node project
    assert_eq!(
        json["target"]["kind"].as_str(),
        Some("project"),
        "Should detect as a project"
    );
    
    // Exit code 0 or 1 (warnings) is OK for this test
    assert!(
        output.status.code() == Some(0) || output.status.code() == Some(1),
        "Command should succeed or have warnings, got: {:?}",
        output.status.code()
    );
}

#[test]
fn typescript_fixture_parses() {
    use std::fs;
    use std::path::Path;
    
    // Create a temporary TypeScript fixture
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path();
    
    // Create package.json
    fs::write(
        temp_path.join("package.json"),
        r#"{
  "name": "ts-test",
  "version": "1.0.0",
  "bin": {
    "ts-test": "src/cli.ts"
  }
}"#,
    )
    .unwrap();
    
    // Create src directory
    fs::create_dir(temp_path.join("src")).unwrap();
    
    // Create a TypeScript file
    fs::write(
        temp_path.join("src/cli.ts"),
        r#"#!/usr/bin/env node
interface Options {
  verbose: boolean;
}

async function main(opts: Options) {
  if (opts.verbose) {
    console.log("Running in verbose mode");
  }
}

main({ verbose: false });
"#,
    )
    .unwrap();
    
    let mut cmd = Command::cargo_bin("anc").unwrap();
    cmd.arg(temp_path).arg("--output").arg("json");
    
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    
    // Parse the JSON output
    let json: serde_json::Value = serde_json::from_str(&stdout)
        .expect("Failed to parse JSON output");
    
    // Check that we detected a Node project (TypeScript uses package.json)
    assert_eq!(
        json["target"]["kind"].as_str(),
        Some("project"),
        "Should detect as a project"
    );
    
    // Exit code 0 or 1 (warnings) is OK for this test
    assert!(
        output.status.code() == Some(0) || output.status.code() == Some(1),
        "Command should succeed or have warnings, got: {:?}",
        output.status.code()
    );
}
