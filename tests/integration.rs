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

// ── Check subcommand tests ─────────────────────────────────────────

#[test]
fn test_check_self() {
    // Running against the project root should produce warnings (exit 1) or failures (exit 2).
    // Either way, it should not exit 0 (we know agentnative has warnings from dogfooding).
    let assert = cmd().args(["check", "."]).assert();

    // Exit code 1 (warnings) or 2 (failures) — not 0
    assert
        .code(predicate::in_iter([1, 2]))
        .stdout(predicate::str::contains("checks:"));
}

#[test]
fn test_check_json_output() {
    let assert = cmd().args(["check", ".", "--output", "json"]).assert();

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
fn test_check_quiet() {
    let normal = cmd()
        .args(["check", "."])
        .output()
        .expect("normal run should succeed");
    let quiet = cmd()
        .args(["check", ".", "-q"])
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
fn test_check_principle_filter() {
    let assert = cmd()
        .args(["check", ".", "--principle", "3", "--output", "json"])
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
fn test_check_nonexistent_path() {
    cmd()
        .args(["check", "/nonexistent/path/that/does/not/exist"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("error"));
}

#[test]
fn test_check_bogus_flag() {
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
        .args(["check", ".", "--output", "json"])
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

    let assert = cmd().args(["check", &path]).assert();

    let output = assert.get_output().stdout.clone();
    let stdout = String::from_utf8(output).expect("stdout should be valid UTF-8");

    // Should run behavioral checks (the shell script is an executable)
    assert!(
        stdout.contains("checks:"),
        "output should contain a checks summary line"
    );

    // Should NOT contain source-layer checks since there is no project directory
    assert!(
        !stdout.contains("source"),
        "binary-only fixture should not run source checks; got:\n{stdout}"
    );
}

#[test]
fn test_source_only_fixture() {
    let path = fixture_path("source-only");

    let assert = cmd().args(["check", &path, "--output", "json"]).assert();

    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("stdout should be valid UTF-8");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("output should be valid JSON");

    let results = parsed["results"]
        .as_array()
        .expect("results should be an array");

    // Should have source and project checks but no behavioral checks
    let has_source = results
        .iter()
        .any(|r| r["layer"].as_str() == Some("source"));
    let has_project = results
        .iter()
        .any(|r| r["layer"].as_str() == Some("project"));
    let has_behavioral = results
        .iter()
        .any(|r| r["layer"].as_str() == Some("behavioral"));

    assert!(has_source, "source-only fixture should have source checks");
    assert!(
        has_project,
        "source-only fixture should have project checks"
    );
    assert!(
        !has_behavioral,
        "source-only fixture should NOT have behavioral checks"
    );
}

#[test]
fn test_broken_fixture() {
    let path = fixture_path("broken-rust");

    let assert = cmd().args(["check", &path, "--output", "json"]).assert();

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

    let assert = cmd().args(["check", &path, "--output", "json"]).assert();

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
    // Bare invocation (no subcommand) must print help and exit 0, not run `check .`.
    // This is enforced by clap's arg_required_else_help and is critical for safe
    // dogfooding — without it, NonInteractiveCheck's bare probe triggers a full
    // recursive check suite.
    cmd()
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Usage"));
}

// ── Default subcommand tests ──────────────────────────────────────
//
// `anc .` should behave like `anc check .` via pre-parse injection.
// The injection must NOT fire for bare invocation, --help, or --version.

#[test]
fn test_default_subcommand_dot_matches_explicit_check() {
    // `anc .` and `anc check .` must produce the same JSON scorecard.
    let implicit = cmd()
        .args([".", "--output", "json"])
        .output()
        .expect("implicit run should execute");
    let explicit = cmd()
        .args(["check", ".", "--output", "json"])
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
        .code(predicate::in_iter([1, 2]))
        .stdout(predicate::str::contains("[PASS]").not())
        .stdout(predicate::str::contains("[SKIP]").not());
}

#[test]
fn test_default_subcommand_preserves_global_long_flag_before_path() {
    // `anc --quiet .` — long-form global flag precedes the path.
    cmd()
        .args(["--quiet", "."])
        .assert()
        .code(predicate::in_iter([1, 2]))
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
    // `anc check .` — must still pass through unchanged.
    cmd()
        .args(["check", "."])
        .assert()
        .code(predicate::in_iter([1, 2]))
        .stdout(predicate::str::contains("checks:"));
}

// ── --command flag tests ──────────────────────────────────────────
//
// `anc --command <name>` resolves a binary from PATH and runs
// behavioral-only checks against it.

#[test]
fn test_command_flag_resolves_path_and_runs_behavioral_only() {
    // `anc check --command ls` — runs behavioral checks against /bin/ls.
    // ls is on every POSIX system, so this is safe to rely on in CI.
    #[cfg(unix)]
    {
        let assert = cmd()
            .args(["check", "--command", "ls", "--output", "json"])
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
fn test_command_flag_via_default_subcommand() {
    // `anc --command ls` — default-subcommand injection yields
    // `anc check --command ls` which runs behavioral checks.
    #[cfg(unix)]
    {
        cmd()
            .args(["--command", "ls"])
            .assert()
            .code(predicate::in_iter([0, 1, 2]))
            .stdout(predicate::str::contains("checks:"));
    }
}

#[test]
fn test_command_flag_unknown_binary_errors() {
    cmd()
        .args(["check", "--command", "this-binary-does-not-exist-xyz-12345"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "command 'this-binary-does-not-exist-xyz-12345' not found on PATH",
        ));
}

#[test]
fn test_command_flag_conflicts_with_path() {
    // `anc check --command ls .` — clap rejects both arguments.
    cmd()
        .args(["check", "--command", "ls", "."])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn test_command_flag_appears_in_help() {
    cmd()
        .args(["check", "--help"])
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
        .args(["check", "--command", "ls", "--source"])
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
        .args(["help", "check"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Resolve a command from PATH"));
}

// ── No-positional flag-only invocations ──────────────────────────
//
// Subcommand-scoped flags imply `check` even with no positional argument.
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
fn test_subcommand_flag_alone_injects_check() {
    // `anc --command ls` — no positional, but `--command` is subcommand-scoped
    // so injection fires. Without this, clap would reject `--command` at the
    // top level.
    #[cfg(unix)]
    {
        cmd()
            .args(["--command", "ls"])
            .assert()
            .code(predicate::in_iter([0, 1, 2]))
            .stdout(predicate::str::contains("checks:"));
    }
}

// ── Flag-value pairing ───────────────────────────────────────────
//
// Tokens following a value-taking flag are values, NOT subcommand candidates.
// `anc --command check` must resolve "check" as a binary name on PATH, not
// route to the explicit `check` subcommand.

#[test]
fn test_command_flag_value_matching_subcommand_name() {
    // `anc --command check` — `check` is the value of `--command`. Should
    // try to resolve a binary named "check" on PATH (which doesn't exist on
    // a typical system) and surface a clean "not found" error.
    cmd()
        .args(["--command", "check"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "command 'check' not found on PATH",
        ));
}

// ── POSIX `--` separator ─────────────────────────────────────────

#[test]
fn test_double_dash_separator_with_path() {
    // `anc -- .` should run check against `.` (POSIX convention treats
    // everything after `--` as positional).
    cmd()
        .args(["--", "."])
        .assert()
        .code(predicate::in_iter([1, 2]))
        .stdout(predicate::str::contains("checks:"));
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

    let assert = cmd().args(["check", &path, "--output", "json"]).assert();

    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("stdout should be valid UTF-8");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("output should be valid JSON");

    let results = parsed["results"]
        .as_array()
        .expect("results should be an array");

    // Should have source-layer checks
    let has_source = results
        .iter()
        .any(|r| r["layer"].as_str() == Some("source"));
    assert!(
        has_source,
        "broken-python fixture should have source checks"
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

    // Specifically check bare-except fires
    let has_bare_except = results.iter().any(|r| {
        r["id"].as_str() == Some("code-bare-except") && r["status"].as_str() == Some("fail")
    });
    assert!(
        has_bare_except,
        "broken-python fixture should trigger code-bare-except check"
    );
}

/// Convention enforcement: check_x() functions must return CheckStatus, not CheckResult.
///
/// The Check trait's run() method is the sole constructor of CheckResult. If check_x()
/// returns CheckResult, the ID/group/layer fields are duplicated as string literals
/// instead of derived from self.id()/self.group()/self.layer(). This test walks the
/// source to catch regressions. Pure Rust — no external tooling required.
#[test]
fn convention_check_x_returns_check_status_not_check_result() {
    use std::ffi::OsStr;
    use std::fs;
    use std::path::Path;

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/checks/source");
    assert!(
        root.is_dir(),
        "source checks dir not found: {}",
        root.display()
    );

    let mut violations: Vec<String> = Vec::new();
    visit_dir(&root, &mut |path, contents| {
        // Match `fn check_<anything>(...) -> CheckResult` allowing any visibility
        // modifier and either single-line or multi-line signatures.
        // We search for `fn check_` then look ahead for `-> CheckResult` before the
        // next `{` (end of signature).
        for (i, line) in contents.lines().enumerate() {
            if let Some(idx) = line.find("fn check_") {
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
                if sig_end.contains("-> CheckResult") {
                    violations.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                }
            }
        }
    });

    assert!(
        violations.is_empty(),
        "Found {} check_x() function(s) returning CheckResult instead of CheckStatus. \
         See CLAUDE.md 'Source Check Convention' — check_x() must return CheckStatus \
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
