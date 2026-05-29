use assert_cmd::Command;
use predicates::prelude::*;

/// Helper to build a Command for the anc binary.
fn cmd() -> Command {
    Command::cargo_bin("anc").expect("binary should exist")
}

/// Helper to get the path to a fixture relative to the project root.
fn fixture_path(name: &str) -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{manifest_dir}/tests/fixtures/{name}")
}

// ── Basic CLI tests ────────────────────────────────────────────────

#[test]
fn test_version() {
    cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("anc"));
}

#[test]
fn test_help() {
    cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}

// ── Emit subcommand ────────────────────────────────────────────

#[test]
fn test_emit_coverage_matrix_writes_artifacts() {
    let dir = integration_tempdir();
    let md = dir.join("matrix.md");
    let json = dir.join("matrix.json");

    cmd()
        .args([
            "emit",
            "coverage-matrix",
            "--out",
            md.to_str().expect("utf8 path"),
            "--json-out",
            json.to_str().expect("utf8 path"),
        ])
        .assert()
        .success();

    let md_content = std::fs::read_to_string(&md).expect("matrix.md written");
    assert!(md_content.contains("# Coverage Matrix"));
    assert!(md_content.contains("P1: Non-Interactive by Default"));

    let json_content = std::fs::read_to_string(&json).expect("matrix.json written");
    let parsed: serde_json::Value = serde_json::from_str(&json_content).expect("valid JSON");
    assert_eq!(parsed["schema_version"], "1.0");
    assert!(parsed["rows"].as_array().expect("rows array").len() >= 40);
}

#[test]
fn test_emit_coverage_matrix_drift_check_passes_on_committed_artifacts() {
    // Running --check against the committed docs/coverage-matrix.md +
    // coverage/matrix.json must pass. If this fails, the registry or a
    // audit's covers() drifted without the artifacts being regenerated.
    cmd()
        .args(["emit", "coverage-matrix", "--check"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .assert()
        .success();
}

fn integration_tempdir() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "anc-integration-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).expect("create tempdir");
    root
}

// ── Audit subcommand tests ─────────────────────────────────────────

#[test]
fn test_audit_self() {
    // Running against the project root should produce a parseable scorecard.
    // anc passes its own dogfood as of the PR3 cleanup, so exit code is 0;
    // earlier in development it returned 1 (warnings) or 2 (failures). All
    // three are valid signals that the run completed normally — the
    // assertion targets the summary line shape, not a specific verdict.
    let assert = cmd().args(["audit", "."]).assert();

    assert
        .code(predicate::in_iter([0, 1, 2]))
        .stdout(predicate::str::contains("audits:"));
}

#[test]
fn test_audit_json_output() {
    let assert = cmd().args(["audit", ".", "--output", "json"]).assert();

    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("stdout should be valid UTF-8");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("output should be valid JSON");

    assert!(
        parsed.get("results").is_some(),
        "JSON should have 'results' key"
    );
    assert!(
        parsed.get("summary").is_some(),
        "JSON should have 'summary' key"
    );
}

#[test]
fn test_audit_quiet() {
    let normal = cmd()
        .args(["audit", "."])
        .output()
        .expect("normal run should succeed");
    let quiet = cmd()
        .args(["audit", ".", "-q"])
        .output()
        .expect("quiet run should succeed");

    let normal_stdout = String::from_utf8_lossy(&normal.stdout);
    let quiet_stdout = String::from_utf8_lossy(&quiet.stdout);

    // Quiet output should be shorter (no PASS/SKIP lines)
    assert!(
        quiet_stdout.len() < normal_stdout.len(),
        "quiet output ({} bytes) should be shorter than normal output ({} bytes)",
        quiet_stdout.len(),
        normal_stdout.len()
    );

    // Quiet output should not contain PASS or SKIP lines
    assert!(
        !quiet_stdout.contains("[PASS]"),
        "quiet output should not contain [PASS] lines"
    );
    assert!(
        !quiet_stdout.contains("[SKIP]"),
        "quiet output should not contain [SKIP] lines"
    );
}

#[test]
fn test_audit_principle_filter() {
    let assert = cmd()
        .args(["audit", ".", "--principle", "3", "--output", "json"])
        .assert();

    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("stdout should be valid UTF-8");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("output should be valid JSON");

    let results = parsed["results"]
        .as_array()
        .expect("results should be an array");

    // All results should be P3, CodeQuality, or ProjectStructure
    for result in results {
        let group = result["group"].as_str().expect("group should be a string");
        assert!(
            group == "P3" || group == "CodeQuality" || group == "ProjectStructure",
            "unexpected group in --principle 3 output: {group}"
        );
    }
}

// ── Error handling tests ───────────────────────────────────────────

#[test]
fn test_audit_nonexistent_path() {
    cmd()
        .args(["audit", "/nonexistent/path/that/does/not/exist"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("error"));
}

#[test]
fn test_audit_bogus_flag() {
    cmd()
        .arg("--bogus-flag")
        .assert()
        .code(2)
        .stderr(predicate::str::is_empty().not());
}

// ── Completions test ───────────────────────────────────────────────

#[test]
fn test_completions_bash() {
    cmd()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

// ── Environment variable test ──────────────────────────────────────

#[test]
fn test_no_color_env() {
    let assert = cmd()
        .env("NO_COLOR", "1")
        .args(["audit", ".", "--output", "json"])
        .assert();

    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("stdout should be valid UTF-8");

    // Should not contain ANSI escape codes
    assert!(
        !json_str.contains("\x1b["),
        "JSON output should not contain ANSI escape codes when NO_COLOR=1"
    );

    // Should still be valid JSON
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("output should be valid JSON with NO_COLOR=1");
    assert!(
        parsed.get("results").is_some(),
        "JSON should have 'results' key"
    );
}

// ── Fixture tests ──────────────────────────────────────────────────

#[test]
fn test_binary_only_fixture() {
    let path = fixture_path("binary-only/test.sh");

    let assert = cmd().args(["audit", &path]).assert();

    let output = assert.get_output().stdout.clone();
    let stdout = String::from_utf8(output).expect("stdout should be valid UTF-8");

    // Should run behavioral audits (the shell script is an executable)
    assert!(
        stdout.contains("audits:"),
        "output should contain an audits summary line"
    );

    // Should NOT contain source-layer audits since there is no project directory
    assert!(
        !stdout.contains("source"),
        "binary-only fixture should not run source audits; got:\n{stdout}"
    );
}

#[test]
fn test_source_only_fixture() {
    let path = fixture_path("source-only");

    let assert = cmd().args(["audit", &path, "--output", "json"]).assert();

    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("stdout should be valid UTF-8");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("output should be valid JSON");

    let results = parsed["results"]
        .as_array()
        .expect("results should be an array");

    // Should have source and project audits but no behavioral audits
    let has_source = results
        .iter()
        .any(|r| r["layer"].as_str() == Some("source"));
    let has_project = results
        .iter()
        .any(|r| r["layer"].as_str() == Some("project"));
    let has_behavioral = results
        .iter()
        .any(|r| r["layer"].as_str() == Some("behavioral"));

    assert!(has_source, "source-only fixture should have source audits");
    assert!(
        has_project,
        "source-only fixture should have project audits"
    );
    assert!(
        !has_behavioral,
        "source-only fixture should NOT have behavioral audits"
    );
}

#[test]
fn test_broken_fixture() {
    let path = fixture_path("broken-rust");

    let assert = cmd().args(["audit", &path, "--output", "json"]).assert();

    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("stdout should be valid UTF-8");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("output should be valid JSON");

    let results = parsed["results"]
        .as_array()
        .expect("results should be an array");

    // Should have failures — broken-rust has .unwrap(), naked println, etc.
    let fail_count = results
        .iter()
        .filter(|r| r["status"].as_str() == Some("fail"))
        .count();
    let warn_count = results
        .iter()
        .filter(|r| r["status"].as_str() == Some("warn"))
        .count();

    assert!(
        fail_count + warn_count > 0,
        "broken-rust fixture should have failures or warnings, got 0"
    );
}

#[test]
fn test_perfect_fixture() {
    let path = fixture_path("perfect-rust");

    let assert = cmd().args(["audit", &path, "--output", "json"]).assert();

    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("stdout should be valid UTF-8");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("output should be valid JSON");

    let summary = &parsed["summary"];
    let fail_count = summary["fail"].as_u64().unwrap_or(999);
    let error_count = summary["error"].as_u64().unwrap_or(999);

    assert_eq!(fail_count, 0, "perfect-rust fixture should have 0 failures");
    assert_eq!(error_count, 0, "perfect-rust fixture should have 0 errors");
}

// ── Bare invocation test ──────────────────────────────────────────

#[test]
fn test_bare_invocation_prints_help() {
    // Bare invocation (no subcommand) must print help and exit 0, not run `audit .`.
    // This is enforced by clap's arg_required_else_help and is critical for safe
    // dogfooding — without it, NonInteractiveAudit's bare probe triggers a full
    // recursive audit suite.
    cmd()
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Usage"));
}

// ── Default subcommand tests ──────────────────────────────────────
//
// `anc .` should behave like `anc audit .` via pre-parse injection.
// The injection must NOT fire for bare invocation, --help, or --version.

#[test]
fn test_default_subcommand_dot_matches_explicit_audit() {
    // `anc .` and `anc audit .` must produce the same JSON scorecard.
    let implicit = cmd()
        .args([".", "--output", "json"])
        .output()
        .expect("implicit run should execute");
    let explicit = cmd()
        .args(["audit", ".", "--output", "json"])
        .output()
        .expect("explicit run should execute");

    assert_eq!(
        implicit.status.code(),
        explicit.status.code(),
        "exit codes must match"
    );

    let implicit_json: serde_json::Value =
        serde_json::from_slice(&implicit.stdout).expect("implicit output must be valid JSON");
    let explicit_json: serde_json::Value =
        serde_json::from_slice(&explicit.stdout).expect("explicit output must be valid JSON");

    assert_eq!(
        implicit_json["summary"], explicit_json["summary"],
        "summaries from implicit and explicit invocations must match"
    );
}

#[test]
fn test_default_subcommand_preserves_global_flag_before_path() {
    // `anc -q .` — global flag precedes the path argument.
    cmd()
        .args(["-q", "."])
        .assert()
        .code(predicate::in_iter([0, 1, 2]))
        .stdout(predicate::str::contains("[PASS]").not())
        .stdout(predicate::str::contains("[SKIP]").not());
}

#[test]
fn test_default_subcommand_preserves_global_long_flag_before_path() {
    // `anc --quiet .` — long-form global flag precedes the path.
    cmd()
        .args(["--quiet", "."])
        .assert()
        .code(predicate::in_iter([0, 1, 2]))
        .stdout(predicate::str::contains("[PASS]").not());
}

#[test]
fn test_default_subcommand_passes_trailing_flags_through() {
    // `anc . --output json` — the injected subcommand must carry trailing flags.
    let assert = cmd().args([".", "--output", "json"]).assert();
    let output = assert.get_output().stdout.clone();
    let parsed: serde_json::Value =
        serde_json::from_slice(&output).expect("output should be valid JSON");
    assert!(parsed.get("results").is_some());
    assert!(parsed.get("summary").is_some());
}

#[test]
fn test_default_subcommand_rejects_nonexistent_path() {
    // `anc /nonexistent/path` — injection runs, clap parses, discover errors.
    cmd()
        .arg("/nonexistent/path/that/does/not/exist")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("error"));
}

#[test]
fn test_default_subcommand_does_not_fire_for_bare_flags() {
    // `anc --help` — no injection; clap renders top-level help.
    cmd()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}

#[test]
fn test_default_subcommand_does_not_fire_for_version() {
    // `anc --version` — no injection; clap prints version.
    cmd()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("anc"));
}

#[test]
fn test_explicit_subcommand_still_works() {
    // `anc audit .` — must still pass through unchanged.
    cmd()
        .args(["audit", "."])
        .assert()
        .code(predicate::in_iter([0, 1, 2]))
        .stdout(predicate::str::contains("audits:"));
}

// ── --command flag tests ──────────────────────────────────────────
//
// `anc --command <name>` resolves a binary from PATH and runs
// behavioral-only audits against it.

#[test]
fn test_command_flag_resolves_path_and_runs_behavioral_only() {
    // `anc audit --command ls` — runs behavioral audits against /bin/ls.
    // ls is on every POSIX system, so this is safe to rely on in CI.
    #[cfg(unix)]
    {
        let assert = cmd()
            .args(["audit", "--command", "ls", "--output", "json"])
            .assert();
        let output = assert.get_output().stdout.clone();
        let parsed: serde_json::Value =
            serde_json::from_slice(&output).expect("output should be valid JSON");
        let results = parsed["results"]
            .as_array()
            .expect("results should be an array");
        assert!(!results.is_empty(), "should have behavioral results");
        // Behavioral-only: no source or project layers.
        for r in results {
            let layer = r["layer"].as_str().unwrap_or("");
            assert_eq!(
                layer, "behavioral",
                "--command should produce only behavioral results, got {layer}"
            );
        }
    }
}

#[test]
fn test_text_and_json_agree_on_row_count_and_exit_code() {
    // Parity guard for the text-renderer fix: the default text view and the
    // JSON view must derive the same per-row set from one pipeline. If
    // main::run ever feeds the text renderer raw probe results again, the
    // text row count (probe-keyed) diverges from JSON results.len()
    // (requirement-keyed), and the two exit codes can disagree. ls is on
    // every POSIX system, so this is safe to rely on in CI.
    #[cfg(unix)]
    {
        let json_assert = cmd()
            .args(["audit", "--command", "ls", "--output", "json"])
            .assert();
        let json_exit = json_assert.get_output().status.code();
        let json_out = json_assert.get_output().stdout.clone();
        let parsed: serde_json::Value =
            serde_json::from_slice(&json_out).expect("output should be valid JSON");
        let json_rows = parsed["results"]
            .as_array()
            .expect("results should be an array")
            .len();

        let text_assert = cmd().args(["audit", "--command", "ls"]).assert();
        let text_exit = text_assert.get_output().status.code();
        let text_out = String::from_utf8(text_assert.get_output().stdout.clone())
            .expect("text output should be utf8");
        // Status-prefixed rows are the only lines that open with two spaces
        // then a bracket; group headers, the summary, evidence (deeper
        // indent), and the badge hint do not.
        let text_rows = text_out.lines().filter(|l| l.starts_with("  [")).count();

        assert_eq!(
            text_rows, json_rows,
            "text row count must equal JSON results.len(); text output:\n{text_out}",
        );
        assert_eq!(
            text_exit, json_exit,
            "exit code must agree between text and JSON modes",
        );
    }
}

#[test]
fn test_command_flag_via_default_subcommand() {
    // `anc --command ls` — default-subcommand injection yields
    // `anc audit --command ls` which runs behavioral audits.
    #[cfg(unix)]
    {
        cmd()
            .args(["--command", "ls"])
            .assert()
            .code(predicate::in_iter([0, 1, 2]))
            .stdout(predicate::str::contains("audits:"));
    }
}

#[test]
fn test_command_flag_unknown_binary_errors() {
    cmd()
        .args(["audit", "--command", "this-binary-does-not-exist-xyz-12345"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "command 'this-binary-does-not-exist-xyz-12345' not found on PATH",
        ));
}

#[test]
fn test_command_flag_conflicts_with_path() {
    // `anc audit --command ls .` — clap rejects both arguments.
    cmd()
        .args(["audit", "--command", "ls", "."])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn test_command_flag_appears_in_help() {
    cmd()
        .args(["audit", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--command"));
}

#[test]
fn test_command_flag_conflicts_with_source() {
    // `--command` and `--source` are contradictory: --command targets a binary
    // (no source code available); --source asks to skip behavioral and run
    // source-only. Clap rejects the combination at parse time.
    cmd()
        .args(["audit", "--command", "ls", "--source"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

// ── help subcommand ──────────────────────────────────────────────
//
// `anc help` and `anc help <subcommand>` are clap-auto-generated and the
// universal CLI convention (cargo, git, npm, kubectl, gh, docker). The
// default-subcommand injection must NOT swallow `help` as a path.

#[test]
fn test_help_subcommand_works() {
    cmd()
        .arg("help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}

#[test]
fn test_help_subcommand_with_target() {
    cmd()
        .args(["help", "audit"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Resolve a command from PATH"));
}

// ── No-positional flag-only invocations ──────────────────────────
//
// Subcommand-scoped flags imply `audit` even with no positional argument.
// Top-level flags do not — `anc -q` prints help and exits 2 (not panic).

#[test]
fn test_quiet_flag_alone_exits_2_not_panic() {
    // PRE-EXISTING bug fix: `anc -q` previously hit `unreachable!()` and
    // panicked (SIGABRT, exit 134). Now the None arm prints help and exits 2.
    cmd()
        .arg("-q")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn test_quiet_long_flag_alone_exits_2() {
    cmd()
        .arg("--quiet")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn test_subcommand_flag_alone_injects_audit() {
    // `anc --command ls` — no positional, but `--command` is subcommand-scoped
    // so injection fires. Without this, clap would reject `--command` at the
    // top level.
    #[cfg(unix)]
    {
        cmd()
            .args(["--command", "ls"])
            .assert()
            .code(predicate::in_iter([0, 1, 2]))
            .stdout(predicate::str::contains("audits:"));
    }
}

// ── Flag-value pairing ───────────────────────────────────────────
//
// Tokens following a value-taking flag are values, NOT subcommand candidates.
// `anc --command audit` must resolve "audit" as a binary name on PATH,
// not route to the explicit `audit` subcommand.

#[test]
fn test_command_flag_value_matching_subcommand_name() {
    // `anc --command audit` — `audit` is the value of `--command`. Should
    // try to resolve a binary named "audit" on PATH (which doesn't exist on
    // a typical system) and surface a clean "not found" error.
    cmd()
        .args(["--command", "audit"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "command 'audit' not found on PATH",
        ));
}

// ── POSIX `--` separator ─────────────────────────────────────────

#[test]
fn test_double_dash_separator_with_path() {
    // `anc -- .` should run audit against `.` (POSIX convention treats
    // everything after `--` as positional).
    cmd()
        .args(["--", "."])
        .assert()
        .code(predicate::in_iter([0, 1, 2]))
        .stdout(predicate::str::contains("audits:"));
}

#[test]
fn test_explicit_completions_subcommand_still_works() {
    // `anc completions bash` — must pass through, not be treated as default subcommand.
    cmd()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

// ── Python fixture tests ────────────────────────────────────────

#[test]
fn test_broken_python_fixture() {
    let path = fixture_path("broken-python");

    let assert = cmd().args(["audit", &path, "--output", "json"]).assert();

    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("stdout should be valid UTF-8");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("output should be valid JSON");

    let results = parsed["results"]
        .as_array()
        .expect("results should be an array");

    // Should have source-layer audits
    let has_source = results
        .iter()
        .any(|r| r["layer"].as_str() == Some("source"));
    assert!(
        has_source,
        "broken-python fixture should have source audits"
    );

    // Should have at least one failure
    let fail_count = results
        .iter()
        .filter(|r| r["status"].as_str() == Some("fail"))
        .count();
    assert!(
        fail_count > 0,
        "broken-python fixture should have at least one failure, got 0"
    );

    // Specifically audit bare-except fires
    let has_bare_except = results.iter().any(|r| {
        r["id"].as_str() == Some("code-bare-except") && r["status"].as_str() == Some("fail")
    });
    assert!(
        has_bare_except,
        "broken-python fixture should trigger code-bare-except audit"
    );
}

/// Convention enforcement: audit_x() functions must return AuditStatus, not AuditResult.
///
/// The Audit trait's run() method is the sole constructor of AuditResult. If audit_x()
/// returns AuditResult, the ID/group/layer fields are duplicated as string literals
// ── --audit-profile tests ─────────────────────────────────────────

#[test]
fn test_audit_profile_rejects_unknown_value() {
    // clap's value_enum rejects unknown values with exit code 2 — no
    // custom validation needed.
    cmd()
        .args(["audit", ".", "--audit-profile", "not-a-real-category"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("audit-profile"));
}

#[test]
fn test_audit_profile_echoed_in_json_output() {
    let assert = cmd()
        .args([
            "audit",
            ".",
            "--audit-profile",
            "human-tui",
            "--output",
            "json",
        ])
        .assert();
    // Non-zero exit OK — anc has warnings on itself; we're asserting the
    // scorecard shape, not the pass/fail verdict.
    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("utf8 stdout");
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");
    assert_eq!(parsed["audit_profile"], "human-tui");
    assert_eq!(parsed["schema_version"], "0.7");
}

#[test]
fn test_audit_profile_suppresses_listed_audits() {
    let assert = cmd()
        .args([
            "audit",
            ".",
            "--audit-profile",
            "human-tui",
            "--output",
            "json",
        ])
        .assert();
    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("utf8 stdout");
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");

    // Suppressed audits appear in results[] with Skip + structured evidence
    // so readers see what was excluded.
    let results = parsed["results"].as_array().expect("results is array");
    let suppressed: Vec<&serde_json::Value> = results
        .iter()
        .filter(|r| {
            r["evidence"]
                .as_str()
                .is_some_and(|s| s.contains("suppressed by audit_profile: human-tui"))
        })
        .collect();

    assert!(
        !suppressed.is_empty(),
        "expected at least one audit suppressed by human-tui profile, got results: {results:?}",
    );
    // Every suppressed result must be status=skip, not a fresh verdict.
    for r in &suppressed {
        assert_eq!(r["status"], "skip", "suppressed audit must be status=skip");
    }
}

#[test]
fn test_audit_profile_absent_emits_null() {
    // No --audit-profile flag: scorecard should echo null, preserving
    // v0.1.2 behavior for consumers that feature-detect.
    let assert = cmd().args(["audit", ".", "--output", "json"]).assert();
    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("utf8 stdout");
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");
    assert!(parsed["audit_profile"].is_null());
}

#[test]
fn test_audit_profile_shrinks_audience_denominator() {
    // When --audit-profile human-tui suppresses p1-non-interactive (one
    // of the 4 audience signal audits), the classifier can't form a 4-way
    // verdict. Expect audience: null per R2.
    let assert = cmd()
        .args([
            "audit",
            ".",
            "--audit-profile",
            "human-tui",
            "--output",
            "json",
        ])
        .assert();
    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("utf8 stdout");
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");
    assert!(
        parsed["audience"].is_null(),
        "audience should be null when a signal audit is suppressed (got {:?})",
        parsed["audience"],
    );
}

#[test]
fn test_audience_non_null_on_self_dogfood() {
    // End-to-end guard for the main.rs → scorecard audience handoff.
    // A unit test in `src/scorecard` feeds synthetic `AuditResult`s into
    // `classify()`, but only an integration run exercises
    // `run_audit()` → `classify()` → `build_scorecard()` on real data.
    // A regression that passes `None` at the main.rs call site would pass
    // every existing unit test and only fail here.
    //
    // Self-dogfood produces `agent-optimized` today — but the specific
    // label isn't the contract. What matters is that `audience` is a
    // non-null string (classifier actually ran) and that `audience_reason`
    // is absent (no gap to explain).
    let assert = cmd().args(["audit", ".", "--output", "json"]).assert();
    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("utf8 stdout");
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");

    let audience = parsed["audience"].as_str().unwrap_or_else(|| {
        panic!(
            "self-dogfood must produce a concrete audience label, got {:?}",
            parsed["audience"]
        )
    });
    assert!(
        matches!(audience, "agent-optimized" | "mixed" | "human-primary"),
        "audience must be one of the 3 enum values, got {audience:?}",
    );
    assert!(
        parsed.get("audience_reason").is_none(),
        "audience_reason key should be absent when audience is labeled, got {}",
        parsed["audience_reason"],
    );
}

#[test]
fn test_principle_filter_forces_audience_null() {
    // `--principle 2` drops every non-P2 audit from `all_audits` before
    // the run loop. Since the 4 audience signals span P1, P2, P6, P7,
    // three of them disappear — audience must be null with
    // `audience_reason: "insufficient_signal"`.
    let assert = cmd()
        .args(["audit", ".", "--principle", "2", "--output", "json"])
        .assert();
    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("utf8 stdout");
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");

    assert!(
        parsed["audience"].is_null(),
        "filtering to a single principle drops 3 of 4 signal audits — audience must be null",
    );
    assert_eq!(parsed["audience_reason"], "insufficient_signal");
}

#[test]
fn test_scorecard_json_has_stable_top_level_keys() {
    // Snapshot-style contract check: the site renderer and any agent
    // consumer pin against this exact key set. A regression that
    // accidentally renames or drops a top-level key fails here rather
    // than silently breaking downstream consumption. New keys (always
    // additive) should add to EXPECTED; removals or renames require a
    // plan revision because they break consumers built against earlier
    // 0.x revisions.
    //
    // Enforcing "no unexpected keys" too (bidirectional check) means an
    // accidental extra key also fails — which is the correct behavior
    // for a versioned schema contract.
    let assert = cmd().args(["audit", ".", "--output", "json"]).assert();
    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("utf8 stdout");
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");
    let obj = parsed.as_object().expect("scorecard is a JSON object");

    const EXPECTED: &[&str] = &[
        "schema_version",
        "results",
        "summary",
        "coverage_summary",
        "audience",
        "audit_profile",
        "spec_version",
        // 0.4 additions — see docs/plans/2026-04-29-001-feat-scorecard-schema-metadata-plan.md.
        "tool",
        "anc",
        "run",
        "target",
        // 0.5 addition — agent-native badge block (eligibility, embed
        // snippet, scorecard/badge URLs).
        "badge",
    ];
    // `audience_reason` is present only when audience is null — on the
    // self-dogfood it should NOT appear, consistent with the skip rule.
    const OPTIONAL: &[&str] = &["audience_reason"];

    for key in EXPECTED {
        assert!(
            obj.contains_key(*key),
            "scorecard JSON missing required key {key:?}; got {:?}",
            obj.keys().collect::<Vec<_>>(),
        );
    }
    let unexpected: Vec<&String> = obj
        .keys()
        .filter(|k| !EXPECTED.contains(&k.as_str()) && !OPTIONAL.contains(&k.as_str()))
        .collect();
    assert!(
        unexpected.is_empty(),
        "scorecard JSON grew unexpected top-level key(s): {unexpected:?}. Additive? \
         Add to EXPECTED/OPTIONAL in this test. Breaking? Plan revision required.",
    );

    // Fixed enumerations also pin against the renderer contract.
    assert_eq!(obj["schema_version"], "0.7");
}

#[test]
fn test_audit_profile_diagnostic_does_not_panic_on_self() {
    // Dogfood edge case from the plan: `diagnostic-only` suppresses
    // p5-dry-run on the self-target. Schema 0.6 emits one result per
    // requirement-row, so the suppressed probe surfaces under its row id
    // `p5-must-dry-run` (covered by the `p5-dry-run` audit) with
    // `audit_id: "p5-dry-run"` for provenance.
    let assert = cmd()
        .args([
            "audit",
            ".",
            "--audit-profile",
            "diagnostic-only",
            "--output",
            "json",
        ])
        .assert()
        .code(predicate::in_iter([0, 1, 2]));
    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("utf8 stdout");
    let parsed: serde_json::Value = serde_json::from_str(&json_str).expect("valid JSON");

    let results = parsed["results"].as_array().expect("results is array");
    let p5 = results
        .iter()
        .find(|r| r["id"] == "p5-must-dry-run" && r["audit_id"] == "p5-dry-run")
        .expect("p5-must-dry-run row (from p5-dry-run probe) should appear in results[]");
    assert_eq!(
        p5["status"], "skip",
        "diagnostic-only must suppress p5-dry-run to Skip (got {p5})",
    );
    let evidence = p5["evidence"]
        .as_str()
        .expect("suppressed p5-must-dry-run carries evidence string");
    assert!(
        evidence.contains("suppressed by audit_profile: diagnostic-only"),
        "expected suppression evidence prefix, got {evidence:?}",
    );
    assert_eq!(parsed["audit_profile"], "diagnostic-only");
}

/// instead of derived from self.id()/self.group()/self.layer(). This test walks the
/// source to catch regressions. Pure Rust — no external tooling required.
#[test]
fn convention_audit_x_returns_audit_status_not_audit_result() {
    use std::ffi::OsStr;
    use std::fs;
    use std::path::Path;

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/audits/source");
    assert!(
        root.is_dir(),
        "source audits dir not found: {}",
        root.display()
    );

    let mut violations: Vec<String> = Vec::new();
    visit_dir(&root, &mut |path, contents| {
        // Match `fn audit_<anything>(...) -> AuditResult` allowing any visibility
        // modifier and either single-line or multi-line signatures.
        // We search for `fn audit_` then look ahead for `-> AuditResult` before the
        // next `{` (end of signature).
        for (i, line) in contents.lines().enumerate() {
            if let Some(idx) = line.find("fn audit_") {
                // Collect signature text until we hit `{` (end of signature)
                let mut sig = String::new();
                let rest = &line[idx..];
                sig.push_str(rest);
                if !sig.contains('{') {
                    // signature continues on following lines
                    for cont in contents.lines().skip(i + 1) {
                        sig.push(' ');
                        sig.push_str(cont);
                        if cont.contains('{') {
                            break;
                        }
                    }
                }
                let sig_end = sig.find('{').map(|e| &sig[..e]).unwrap_or(&sig);
                if sig_end.contains("-> AuditResult") {
                    violations.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                }
            }
        }
    });

    assert!(
        violations.is_empty(),
        "Found {} audit_x() function(s) returning AuditResult instead of AuditStatus. \
         See CLAUDE.md 'Source Audit Convention' — audit_x() must return AuditStatus \
         so run() can use self.id()/self.group()/self.layer() as the sole source of truth.\n\n\
         Violations:\n{}",
        violations.len(),
        violations.join("\n")
    );

    fn visit_dir<F: FnMut(&Path, &str)>(dir: &Path, f: &mut F) {
        for entry in fs::read_dir(dir).expect("read_dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            if path.is_dir() {
                visit_dir(&path, f);
            } else if path.extension() == Some(OsStr::new("rs")) {
                let contents = fs::read_to_string(&path).expect("read_to_string");
                f(&path, &contents);
            }
        }
    }
}

/// CLAUDE.md "Source Audit Convention" says: **no `Audit` impl constructs
/// `AuditResult` outside its own `run()`.** Every audit file's
/// `AuditResult { ... }` struct literal must sit inside a `fn run(`
/// function body — not in helpers, module-level code, or anything else.
///
/// The runtime layer (`src/main.rs`) is exempt — it legitimately
/// constructs `AuditResult` in the error and audit_profile-suppression
/// branches using `audit.id()` / `audit.label()` / `audit.group()` /
/// `audit.layer()` from the trait, not string literals. Test doubles
/// inside `#[cfg(test)]` are also exempt.
///
/// This test walks every `src/audits/**/*.rs` file, strips the
/// `#[cfg(test)]` section, and flags any `AuditResult {` literal whose
/// nearest preceding `fn <name>(` declaration is not named `run`. Paired
/// with `convention_audit_x_returns_audit_status_not_audit_result`
/// above (which catches helpers returning AuditResult), this enforces
/// the full convention.
#[test]
fn convention_audit_result_constructed_only_in_run_body() {
    use std::ffi::OsStr;
    use std::fs;
    use std::path::Path;

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/audits");
    assert!(
        root.is_dir(),
        "audits root dir not found: {}",
        root.display()
    );

    let mut violations: Vec<String> = Vec::new();
    visit_dir_all(&root, &mut |path, contents| {
        // Strip everything at and below the first `#[cfg(test)]` — test
        // modules construct AuditResult via FakeAudit / make_result and
        // are legitimately exempt from the convention.
        let scan = match contents.find("#[cfg(test)]") {
            Some(cut) => &contents[..cut],
            None => contents,
        };

        let lines: Vec<&str> = scan.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            // Match struct-literal construction, not the return-type
            // position in a function signature (`-> AuditResult {`).
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            let idx = match line.find("AuditResult {") {
                Some(k) => k,
                None => continue,
            };
            // Skip the signature return-type case.
            let before = &line[..idx];
            if before.trim_end().ends_with("->") {
                continue;
            }

            // Walk backward to find the nearest `fn <name>(` declaration.
            let enclosing_fn = lines[..i]
                .iter()
                .rev()
                .find_map(|l| {
                    let t = l.trim_start();
                    // Ignore comment lines mentioning `fn foo(`.
                    if t.starts_with("//") {
                        return None;
                    }
                    let pos = t.find("fn ")?;
                    let after = &t[pos + 3..];
                    let name_end = after.find(|c: char| !c.is_ascii_alphanumeric() && c != '_')?;
                    Some(after[..name_end].to_string())
                })
                .unwrap_or_else(|| "<module>".to_string());

            if enclosing_fn != "run" {
                violations.push(format!(
                    "{}:{}: AuditResult constructed inside `fn {enclosing_fn}`, not `fn run`",
                    path.display(),
                    i + 1,
                ));
            }
        }
    });

    assert!(
        violations.is_empty(),
        "Found {} AuditResult construction(s) outside `fn run` in src/audits/. \
         CLAUDE.md 'Source Audit Convention' requires run() to be the sole \
         AuditResult constructor per Audit impl. Move the construction into \
         run() (helpers should return AuditStatus).\n\n\
         Violations:\n{}",
        violations.len(),
        violations.join("\n")
    );

    fn visit_dir_all<F: FnMut(&Path, &str)>(dir: &Path, f: &mut F) {
        for entry in fs::read_dir(dir).expect("read_dir") {
            let entry = entry.expect("entry");
            let path = entry.path();
            if path.is_dir() {
                visit_dir_all(&path, f);
            } else if path.extension() == Some(OsStr::new("rs")) {
                let contents = fs::read_to_string(&path).expect("read_to_string");
                f(&path, &contents);
            }
        }
    }
}

// ── JSON envelope intercept (p2-must-json-errors) ─────────────────

/// Bad invocation under `--output json` MUST emit a JSON envelope with
/// `error`, `kind`, and `message`. Without this, agents that pinned to JSON
/// can't recover failure mode without a separate text parser.
#[test]
fn test_bad_invocation_emits_json_error_envelope() {
    let assert = cmd()
        .args(["--this-flag-does-not-exist-anc", "--output", "json"])
        .assert()
        .code(2);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf8 stderr");
    let line = stderr
        .lines()
        .next()
        .expect("envelope should be on the first stderr line");
    let parsed: serde_json::Value =
        serde_json::from_str(line).expect("first stderr line should parse as JSON");
    let obj = parsed.as_object().expect("envelope is a JSON object");
    assert!(obj.contains_key("error"));
    assert!(obj.contains_key("kind"));
    assert!(obj.contains_key("message"));
    assert_eq!(obj["exit_code"], 2);
}

/// Top-level `--json` alias must trigger the same JSON envelope contract.
#[test]
fn test_bad_invocation_with_json_alias_emits_envelope() {
    let assert = cmd()
        .args(["--json", "--this-flag-does-not-exist-anc"])
        .assert()
        .code(2);

    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf8 stderr");
    let line = stderr.lines().next().expect("envelope on first line");
    let parsed: serde_json::Value = serde_json::from_str(line).expect("valid JSON");
    let obj = parsed.as_object().expect("object");
    assert_eq!(obj["kind"], "usage");
}

/// `--help --output json` must emit a JSON envelope wrapping the help text.
/// Tests the success-envelope arm of `p2-should-consistent-envelope`.
#[test]
fn test_help_under_json_mode_emits_json_envelope() {
    let assert = cmd().args(["--help", "--output", "json"]).assert().code(0);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    let line = stdout
        .lines()
        .next()
        .expect("help envelope on first stdout line");
    let parsed: serde_json::Value = serde_json::from_str(line).expect("valid JSON");
    let obj = parsed.as_object().expect("object");
    assert_eq!(obj["kind"], "help");
    assert!(obj["data"].is_string());
}

/// `--version --output json` must emit a JSON envelope with name + version.
#[test]
fn test_version_under_json_mode_emits_json_envelope() {
    let assert = cmd()
        .args(["--version", "--output", "json"])
        .assert()
        .code(0);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    let line = stdout
        .lines()
        .next()
        .expect("version envelope on first stdout line");
    let parsed: serde_json::Value = serde_json::from_str(line).expect("valid JSON");
    let obj = parsed.as_object().expect("object");
    assert_eq!(obj["kind"], "version");
    assert_eq!(obj["data"]["name"], "anc");
    assert!(obj["data"]["version"].is_string());
}

/// Text mode (no JSON flag) must still produce clap's default rendering for
/// bad invocations — confirms the envelope path is gated on JSON mode.
#[test]
fn test_bad_invocation_without_json_uses_clap_rendering() {
    let assert = cmd()
        .args(["--this-flag-does-not-exist-anc"])
        .assert()
        .code(2);
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf8 stderr");
    assert!(
        stderr.contains("error:"),
        "expected clap text error rendering, got: {stderr}"
    );
    // Ensure the first line is NOT JSON — proves we didn't accidentally
    // route text-mode errors through the envelope.
    let first = stderr.lines().next().unwrap_or("");
    assert!(
        serde_json::from_str::<serde_json::Value>(first).is_err(),
        "text-mode error must not be JSON, but first line parsed: {first}"
    );
}
