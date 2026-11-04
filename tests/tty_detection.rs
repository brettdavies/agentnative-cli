//! The two audits on the TTY side of P1 and P6, end to end: the color audit
//! reports under the requirement it checks, and prompt calls are graded on the
//! stdin check that guards them.

use std::path::Path;

use assert_cmd::Command;

mod common;

/// A one-file Rust crate whose `src/main.rs` is `main_rs`.
fn crate_with_main(main_rs: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        tmp.path().join("Cargo.toml"),
        "[package]\nname = \"tiny\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("write Cargo.toml");
    std::fs::create_dir_all(tmp.path().join("src")).expect("mkdir src");
    std::fs::write(tmp.path().join("src/main.rs"), main_rs).expect("write main.rs");
    tmp
}

/// The result rows of a source-only JSON audit of `dir`.
fn source_rows(dir: &Path, args: &[&str]) -> Vec<serde_json::Value> {
    let output = Command::cargo_bin("anc")
        .expect("binary should exist")
        .env("AGENTNATIVE_HOME_CONFIG", common::empty_home_config())
        .args(["audit", dir.to_str().expect("utf-8 path"), "--source"])
        .args(["--output", "json"])
        .args(args)
        .output()
        .expect("spawn anc");
    let scorecard: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the scorecard is JSON");
    scorecard["results"]
        .as_array()
        .expect("results is an array")
        .clone()
}

fn row_status(rows: &[serde_json::Value], audit_id: &str) -> Option<String> {
    rows.iter()
        .find(|row| row["audit_id"] == audit_id)
        .map(|row| row["status"].as_str().expect("status").to_string())
}

const COLOR_ONLY_MAIN: &str =
    "use colored::Colorize;\nfn main() {\n    println!(\"{}\", \"hi\".green());\n}\n";

#[test]
fn the_color_tty_audit_reports_under_principle_six() {
    let tmp = crate_with_main(COLOR_ONLY_MAIN);

    let six = source_rows(tmp.path(), &["--principle", "6"]);
    assert_eq!(
        row_status(&six, "p1-tty-detection-source").as_deref(),
        Some("warn"),
        "{six:#?}"
    );

    let one = source_rows(tmp.path(), &["--principle", "1"]);
    assert_eq!(
        row_status(&one, "p1-tty-detection-source"),
        None,
        "{one:#?}"
    );
}

#[test]
fn the_coverage_matrix_files_tty_detection_under_the_requirement_each_audit_checks() {
    let matrix: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/coverage/matrix.json"))
            .expect("read coverage/matrix.json"),
    )
    .expect("coverage/matrix.json is JSON");
    let verifiers = |id: &str| -> Vec<String> {
        matrix["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .find(|row| row["id"] == id)
            .expect("the requirement has a row")["verifiers"]
            .as_array()
            .expect("verifiers")
            .iter()
            .map(|verifier| verifier["audit_id"].as_str().expect("audit_id").to_string())
            .collect()
    };

    assert!(
        verifiers("p6-must-no-color").contains(&"p1-tty-detection-source".to_string()),
        "{:?}",
        verifiers("p6-must-no-color")
    );
    assert_eq!(
        verifiers("p1-should-tty-detection"),
        ["p1-prompt-tty-source"]
    );
}

#[test]
fn a_prompt_call_is_graded_on_the_stdin_check_that_guards_it() {
    let guarded = crate_with_main(
        "use dialoguer::Confirm;\nuse std::io::IsTerminal;\nfn main() {\n    if std::io::stdin().is_terminal() {\n        Confirm::new().interact();\n    }\n}\n",
    );
    assert_eq!(
        row_status(&source_rows(guarded.path(), &[]), "p1-prompt-tty-source").as_deref(),
        Some("pass")
    );

    let stdout_only = crate_with_main(
        "use dialoguer::Confirm;\nuse std::io::IsTerminal;\nfn main() {\n    if std::io::stdout().is_terminal() {\n        Confirm::new().interact();\n    }\n}\n",
    );
    assert_eq!(
        row_status(
            &source_rows(stdout_only.path(), &[]),
            "p1-prompt-tty-source"
        )
        .as_deref(),
        Some("warn")
    );

    let color_only = crate_with_main(COLOR_ONLY_MAIN);
    assert_eq!(
        row_status(&source_rows(color_only.path(), &[]), "p1-prompt-tty-source").as_deref(),
        Some("n_a")
    );
}

#[test]
fn the_human_tui_profile_suppresses_the_prompt_guard_audit() {
    let tmp = crate_with_main(
        "use dialoguer::Confirm;\nfn main() {\n    Confirm::new().interact();\n}\n",
    );
    let rows = source_rows(tmp.path(), &["--audit-profile", "human-tui"]);
    assert_eq!(
        row_status(&rows, "p1-prompt-tty-source").as_deref(),
        Some("skip"),
        "{rows:#?}"
    );
}
