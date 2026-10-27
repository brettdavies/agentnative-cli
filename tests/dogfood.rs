//! Dogfood guard: `anc` audits its own repository, and no row may `fail` or
//! `error`. Warnings are tolerated. A linter that grades other tools against
//! the principles has to meet them itself, so a row that breaks here is a
//! product regression, not a style note.
//!
//! The test spawns the real binary in project mode against this repo
//! (CARGO_MANIFEST_DIR) and parses the JSON scorecard, so every layer counts:
//! behavioral, source, and project.

use assert_cmd::Command;
use serde_json::Value;

mod common;

fn cmd() -> Command {
    let mut cmd = Command::cargo_bin("anc").expect("anc binary should exist");
    cmd.env("AGENTNATIVE_HOME_CONFIG", common::empty_home_config());
    cmd
}

fn audit_repo_json() -> Value {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let out = cmd()
        .args(["audit", manifest, "--output", "json"])
        .output()
        .expect("anc audit spawn");
    let stdout = String::from_utf8(out.stdout).expect("utf-8 stdout");
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("failed to parse `anc audit` JSON: {e}\nstdout:\n{stdout}"))
}

/// Every row that failed or errored, as `<id> / <audit id> (<status>): <evidence>`.
fn broken_rows(parsed: &Value) -> Vec<String> {
    parsed["results"]
        .as_array()
        .expect("results array")
        .iter()
        .filter(|r| r["status"] == "fail" || r["status"] == "error")
        .map(|r| {
            format!(
                "{} / {} ({}): {}",
                r["id"].as_str().unwrap_or("?"),
                r["audit_id"].as_str().unwrap_or("?"),
                r["status"].as_str().unwrap_or("?"),
                r["evidence"].as_str().unwrap_or("(no evidence)"),
            )
        })
        .collect()
}

#[test]
fn dogfood_no_row_fails_or_errors_on_this_repo() {
    let broken = broken_rows(&audit_repo_json());
    assert!(
        broken.is_empty(),
        "anc must pass its own audit: no row may fail or error. Broken rows:\n  {}",
        broken.join("\n  "),
    );
}
