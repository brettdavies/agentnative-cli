//! Red-team regression tests for `tool.version` probing.
//!
//! These lock in the security guarantees the v0.4 schema plan promised: a
//! self-spawn declined, hostile-binary survival (memory cap, timeout, nonzero
//! exit). Without these tests, a future refactor could quietly drop the
//! self-spawn guard or replace `BinaryRunner::run` with a fresh
//! `Command::output()` and pass every shape-and-value test in the suite.
//!
//! Plan reference: `docs/plans/2026-04-29-001-feat-scorecard-schema-metadata-plan.md`,
//! Risks table + U5 Test scenarios.

use assert_cmd::Command;
use serde_json::Value;
use std::time::{Duration, Instant};

fn cmd() -> Command {
    Command::cargo_bin("anc").expect("anc binary should exist")
}

fn fixture_path(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Spawn anc, parse its stdout as JSON. The shared output-buffering used by
/// every test below.
fn run_and_parse(args: &[&str]) -> (Value, std::process::Output) {
    let output = cmd()
        .args(args)
        .timeout(Duration::from_secs(20))
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout.clone()).expect("utf-8 stdout");
    let parsed: Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!("anc must emit valid JSON, got error {e}; stdout was {stdout:?}")
    });
    (parsed, output)
}

#[test]
#[cfg(unix)]
fn self_spawn_against_anc_binary_yields_null_version() {
    // Recursive-fork-bomb hazard: pointing anc at its own binary path must
    // hit the self-spawn guard in `probe_tool_version` and emit
    // `tool.version: null`. `arg_required_else_help` already prevents the
    // bare-args recursion path; this guard is defense-in-depth against any
    // future loosening of that contract or any version-probe refactor that
    // forgets the comparison.
    let anc_path = assert_cmd::cargo::cargo_bin("anc");
    let anc_str = anc_path.to_str().expect("utf-8 binary path");

    let (parsed, output) = run_and_parse(&["check", anc_str, "--output", "json"]);

    assert_eq!(
        parsed["target"]["kind"], "binary",
        "self-spawn target is the running binary file, kind must be 'binary'",
    );
    assert!(
        parsed["tool"]["version"].is_null(),
        "self-spawn guard must decline the version probe — \
         got {:?}; full stdout: {}",
        parsed["tool"]["version"],
        String::from_utf8_lossy(&output.stdout),
    );
}

#[test]
#[cfg(unix)]
fn hostile_binary_flooding_stdout_does_not_exhaust_memory() {
    // Fixture emits ~2 MiB on `--version`. The runner's `read_capped`
    // primitive enforces a 1 MiB ceiling. anc must complete the run
    // without crashing or exhausting memory; the captured `tool.version`
    // remains a string-or-null.
    let path = fixture_path("hostile-stdout-flood/probe.sh");
    let start = Instant::now();
    let (parsed, _) = run_and_parse(&["check", &path, "--output", "json"]);
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(10),
        "anc must complete promptly even when target floods stdout; took {elapsed:?}",
    );
    let v = &parsed["tool"]["version"];
    assert!(
        v.is_string() || v.is_null(),
        "tool.version must serialize as string-or-null after stdout flood, got {v:?}",
    );
    if let Some(s) = v.as_str() {
        // The cap prevents memory exhaustion. The captured first line is
        // bounded by what fits before the first newline in the fixture's
        // output. We don't pin an exact size but require it stays sane.
        assert!(
            s.len() < 4 * 1024 * 1024,
            "captured first line must be bounded, got {} bytes",
            s.len(),
        );
    }
}

#[test]
#[cfg(unix)]
fn hostile_binary_that_hangs_is_killed_at_timeout() {
    // Fixture sleeps 30s on `--version`. probe_tool_version's BinaryRunner
    // has a 2-second timeout, so a healthy anc returns in ~2-4s (one
    // timeout per probe attempt: --version then -V). If a regression drops
    // the timeout, the test will time out at assert_cmd's 20s ceiling and
    // fail loudly.
    let path = fixture_path("hostile-hang/probe.sh");
    let start = Instant::now();
    let (parsed, _) = run_and_parse(&["check", &path, "--output", "json"]);
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(15),
        "hung version probe must be killed at 2s timeout; total run took {elapsed:?}",
    );
    assert!(
        parsed["tool"]["version"].is_null(),
        "hung version probe must yield tool.version: null, got {:?}",
        parsed["tool"]["version"],
    );
}

#[test]
#[cfg(unix)]
fn hostile_binary_nonzero_version_exit_yields_null() {
    // Fixture exits 1 on both `--version` and `-V`. Both probe attempts
    // must fail through; tool.version becomes null. The overall run
    // succeeds — a target that refuses to self-report its version is not
    // a scoring error.
    let path = fixture_path("hostile-nonzero-exit/probe.sh");
    let (parsed, _) = run_and_parse(&["check", &path, "--output", "json"]);

    assert!(
        parsed["tool"]["version"].is_null(),
        "every nonzero version probe must yield null, got {:?}",
        parsed["tool"]["version"],
    );
    // The scorecard itself must still emit — version probe failure is not
    // a scoring failure.
    assert_eq!(parsed["schema_version"], "0.4");
    assert_eq!(parsed["target"]["kind"], "binary");
}

#[test]
fn unknown_command_errors_with_actionable_message() {
    // The v0.4 metadata work did NOT change `resolve_command_on_path`'s
    // contract: an unknown command name still produces a top-level error,
    // not a scorecard with null fields. Locking this in defends against a
    // well-meaning refactor that "tolerates" unknown commands by emitting
    // an empty scorecard — which would silently bless an invalid target.
    let assert = cmd()
        .args([
            "check",
            "--command",
            "definitely-not-a-real-cmd-xyzzy-0a8b",
            "--output",
            "json",
        ])
        .timeout(Duration::from_secs(10))
        .assert()
        .failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf-8");
    assert!(
        stderr.contains("not found"),
        "unknown command must explain itself, got stderr: {stderr:?}",
    );
}

#[test]
fn project_mode_without_built_binary_emits_manifest_version_and_null_binary() {
    // `tests/fixtures/perfect-rust` has Cargo.toml but no target/ — the
    // canonical "freshly cloned, never built" state. Plan U5 contract:
    // `tool.binary` is null (no executable to probe), `tool.version` falls
    // through to the manifest's [package].version (0.1.0 in this fixture).
    // A future change that probes a non-existent binary path would emit
    // `tool.version: null` here and fail the assertion.
    let path = fixture_path("perfect-rust");
    let (parsed, _) = run_and_parse(&["check", &path, "--output", "json"]);

    assert_eq!(parsed["target"]["kind"], "project");
    assert!(
        parsed["tool"]["binary"].is_null(),
        "no built binary in fixture — tool.binary must be null, got {:?}",
        parsed["tool"]["binary"],
    );
    assert_eq!(
        parsed["tool"]["version"], "0.1.0",
        "manifest version (Cargo.toml [package].version) must populate tool.version when no binary exists",
    );
}
