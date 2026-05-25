//! Dogfood guards for the new `anc skill install` verb. Tests 24 and 25
//! from the plan, both CRITICAL: without them, the dogfood claim that
//! drove the binary-verb-vs-bash-one-liner decision (Problem Frame § "Why
//! a binary verb, not a bash one-liner?") breaks silently.
//!
//! Each test spawns the real binary in project mode against this repo
//! (CARGO_MANIFEST_DIR), parses the JSON envelope, and asserts no FAIL
//! status on any `p2-*` (test 25) or `p5-*` (test 24) check. Warnings
//! are tolerated; only `fail` breaks the guard.

use assert_cmd::Command;
use serde_json::Value;

fn cmd() -> Command {
    Command::cargo_bin("anc").expect("anc binary should exist")
}

fn check_repo_json() -> Value {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let out = cmd()
        .args(["audit", manifest, "--output", "json"])
        .output()
        .expect("anc check spawn");
    let stdout = String::from_utf8(out.stdout).expect("utf-8 stdout");
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("failed to parse `anc check` JSON: {e}\nstdout:\n{stdout}"))
}

fn collect_failed(parsed: &Value, prefix: &str) -> Vec<String> {
    parsed["results"]
        .as_array()
        .expect("results array")
        .iter()
        .filter(|r| {
            r["id"].as_str().is_some_and(|id| id.starts_with(prefix)) && r["status"] == "fail"
        })
        .map(|r| {
            format!(
                "{} ({})",
                r["id"].as_str().unwrap_or("?"),
                r["evidence"].as_str().unwrap_or("(no evidence)"),
            )
        })
        .collect()
}

/// Test 24 — CRITICAL. P5 (introspection — `--dry-run`, `--print` etc.)
/// must show no `fail` after adding `anc skill install`. The new verb
/// supports `--dry-run`; landing it without dogfooding P5 would invalidate
/// the principle the spec ships against.
#[test]
fn dogfood_no_p5_fail_after_skill_subcommand() {
    let parsed = check_repo_json();
    let failed = collect_failed(&parsed, "p5-");
    assert!(
        failed.is_empty(),
        "p5-* checks must not fail on this repo. Failures:\n  {}",
        failed.join("\n  "),
    );
}

/// Test 25 — CRITICAL. P2 (structured output — `--output {text,json}` and
/// the JSON envelope contract) must show no `fail` after adding the new
/// verb. `anc skill install` was specifically designed to dogfood P2 by
/// emitting an envelope on every outcome.
///
/// **Temporary allowlist** for `p2-schema-print`: the v0.4.0 spec sync
/// added `p2-must-schema-print`, which probes for a `schema` subcommand
/// or `--schema` flag. anc emits structured output but the schema-export
/// surface is yet-unshipped — the planned implementation lives at
/// `docs/plans/2026-04-30-002-feat-scorecard-json-schema-plan.md`
/// (derived schema via `schemars` build-dep, embedded via `include_str!`,
/// exposed via `anc generate scorecard-schema`). Remove this allowlist
/// when that plan's verb lands and satisfies the check.
///
/// **Temporary allowlist** for `p2-json-errors`: the wire-orphan batch
/// that added this check probes the bad-invocation surface for the spec's
/// `error`/`kind`/`message` JSON envelope under `--output json`. anc
/// currently passes argv to clap's default `parse()`, which emits
/// plain-text errors regardless of the active output mode. Honoring
/// `--output json` for parse errors requires switching to `try_parse()`
/// and routing the resulting `clap::Error` through anc's error formatter
/// — a separate feature, not part of the orphan-coverage batch. Remove
/// this allowlist when that work lands.
#[test]
fn dogfood_no_p2_fail_after_skill_subcommand() {
    const PENDING_FAILS: &[&str] = &["p2-schema-print", "p2-json-errors"];

    let parsed = check_repo_json();
    let failed: Vec<String> = collect_failed(&parsed, "p2-")
        .into_iter()
        .filter(|f| !PENDING_FAILS.iter().any(|id| f.contains(id)))
        .collect();
    assert!(
        failed.is_empty(),
        "p2-* checks must not fail on this repo (excluding documented pending: {PENDING_FAILS:?}). \
         Failures:\n  {}",
        failed.join("\n  "),
    );
}
