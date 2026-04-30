//! End-to-end schema 0.5 drift guard.
//!
//! Spawns the real binary in each of the three `anc check` modes (project,
//! binary, command) and asserts the documented v0.5 keys are all present in
//! the JSON output. Catches gaps that unit tests can't — argv capture must
//! actually flow through `inject_default_subcommand`, version probing must
//! actually spawn a child, the `badge` block must be derived from the live
//! tool slug, etc.

use assert_cmd::Command;
use serde_json::Value;

fn cmd() -> Command {
    Command::cargo_bin("anc").expect("anc binary should exist")
}

fn fixture_path(name: &str) -> String {
    format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Assert every documented v0.5 key path resolves on the parsed JSON. The
/// segmented walk gives a precise failure message when a field is missing.
fn assert_v05_shape(parsed: &Value) {
    assert_eq!(
        parsed["schema_version"], "0.5",
        "schema_version must be 0.5",
    );

    for path in [
        // 0.1-0.3 carryover — drift guard against accidental removal.
        "results",
        "summary",
        "coverage_summary",
        "audience",
        "audit_profile",
        "spec_version",
        // 0.4 additions.
        "tool.name",
        "tool.binary",
        "tool.version",
        "anc.version",
        "anc.commit",
        "run.invocation",
        "run.started_at",
        "run.duration_ms",
        "run.platform.os",
        "run.platform.arch",
        "target.kind",
        "target.path",
        "target.command",
        // 0.5 additions — agent-native badge block.
        "badge.eligible",
        "badge.score_pct",
        "badge.embed_markdown",
        "badge.scorecard_url",
        "badge.badge_url",
        "badge.convention_url",
    ] {
        let mut node = parsed;
        for segment in path.split('.') {
            node = node
                .get(segment)
                .unwrap_or_else(|| panic!("expected key `{path}` — segment `{segment}` missing"));
        }
    }

    // The convention URL is fixed and shared across every scored tool. A
    // regression that pointed it at a stale path would silently break the
    // pre-launch surface — pin it loudly here.
    assert_eq!(
        parsed["badge"]["convention_url"], "https://anc.dev/badge",
        "badge.convention_url must be the canonical /badge page",
    );
}

#[test]
fn schema_v05_project_mode_emits_full_shape() {
    let path = fixture_path("perfect-rust");
    let output = cmd()
        .args(["check", &path, "--output", "json"])
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: Value = serde_json::from_str(&stdout).expect("valid JSON");

    assert_v05_shape(&parsed);
    assert_eq!(parsed["target"]["kind"], "project");
    assert!(
        parsed["target"]["path"].is_string(),
        "project mode must populate target.path",
    );
    assert!(parsed["target"]["command"].is_null());
}

#[test]
fn schema_v05_binary_mode_emits_full_shape() {
    let path = fixture_path("binary-only/test.sh");
    let output = cmd()
        .args(["check", &path, "--output", "json"])
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: Value = serde_json::from_str(&stdout).expect("valid JSON");

    assert_v05_shape(&parsed);
    assert_eq!(parsed["target"]["kind"], "binary");
    assert!(parsed["target"]["path"].is_string());
    assert!(parsed["target"]["command"].is_null());
}

#[test]
fn schema_v05_command_mode_emits_full_shape() {
    // `echo` exists on every supported platform; the version probe is
    // best-effort and tolerates whatever `echo --version` happens to print.
    let output = cmd()
        .args(["check", "--command", "echo", "--output", "json"])
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: Value = serde_json::from_str(&stdout).expect("valid JSON");

    assert_v05_shape(&parsed);
    assert_eq!(parsed["target"]["kind"], "command");
    assert!(parsed["target"]["path"].is_null());
    assert_eq!(parsed["target"]["command"], "echo");
    assert_eq!(parsed["tool"]["name"], "echo");
    assert_eq!(parsed["tool"]["binary"], "echo");
}

#[test]
fn schema_v05_run_invocation_captures_user_intent_pre_injection() {
    // Plan R4: a user who typed `anc <path>` (default-subcommand injection)
    // must see `anc <path>` in the scorecard, NOT `anc check <path>`.
    let path = fixture_path("perfect-rust");
    let output = cmd()
        .args([&path, "--output", "json"]) // no explicit `check`
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: Value = serde_json::from_str(&stdout).expect("valid JSON");

    let invocation = parsed["run"]["invocation"]
        .as_str()
        .expect("run.invocation is a string");
    assert!(
        !invocation.contains(" check "),
        "run.invocation must reflect user intent (pre-injection), got: {invocation}",
    );
}

#[test]
fn schema_v05_run_platform_matches_runtime_os_arch() {
    let path = fixture_path("perfect-rust");
    let output = cmd()
        .args(["check", &path, "--output", "json"])
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: Value = serde_json::from_str(&stdout).expect("valid JSON");

    assert_eq!(parsed["run"]["platform"]["os"], std::env::consts::OS);
    assert_eq!(parsed["run"]["platform"]["arch"], std::env::consts::ARCH);
}

#[test]
fn schema_v05_run_started_at_parses_as_rfc3339() {
    let path = fixture_path("perfect-rust");
    let output = cmd()
        .args(["check", &path, "--output", "json"])
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: Value = serde_json::from_str(&stdout).expect("valid JSON");

    let started_at = parsed["run"]["started_at"]
        .as_str()
        .expect("started_at is a string");
    // RFC 3339 shape check without pulling the `time` parsing feature into
    // dev-deps just for one test: `YYYY-MM-DDTHH:MM:SS` plus optional
    // fractional seconds, ending in `Z` or a timezone offset.
    assert!(
        started_at.len() >= 20
            && started_at.as_bytes()[4] == b'-'
            && started_at.as_bytes()[7] == b'-'
            && started_at.as_bytes()[10] == b'T'
            && started_at.as_bytes()[13] == b':'
            && started_at.as_bytes()[16] == b':',
        "started_at must look like RFC 3339, got {started_at:?}",
    );
    let last = started_at.chars().last().expect("non-empty");
    assert!(
        last == 'Z' || started_at.contains('+') || started_at[10..].contains('-'),
        "started_at must end in `Z` or a timezone offset, got {started_at:?}",
    );
}

#[test]
fn schema_v05_anc_version_matches_cargo_pkg_version() {
    let path = fixture_path("perfect-rust");
    let output = cmd()
        .args(["check", &path, "--output", "json"])
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: Value = serde_json::from_str(&stdout).expect("valid JSON");

    assert_eq!(parsed["anc"]["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn schema_v05_badge_block_reflects_live_tool_slug() {
    // End-to-end the `badge.*` URLs are derived from `tool.name` — a
    // regression that hardcodes "demo" or pulls the slug from the wrong
    // place would produce an embed URL that doesn't match the live
    // scorecard page. This pins the slug↔URL relationship without
    // depending on the actual score (a fixture's pass-rate may shift as
    // checks evolve, so we only assert URL shape, not eligibility).
    let path = fixture_path("perfect-rust");
    let output = cmd()
        .args(["check", &path, "--output", "json"])
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: Value = serde_json::from_str(&stdout).expect("valid JSON");

    let slug = parsed["tool"]["name"]
        .as_str()
        .expect("tool.name is a string");
    assert!(!slug.is_empty(), "tool.name must be non-empty");

    let badge_url = parsed["badge"]["badge_url"]
        .as_str()
        .expect("badge.badge_url present when slug derived");
    let scorecard_url = parsed["badge"]["scorecard_url"]
        .as_str()
        .expect("badge.scorecard_url present when slug derived");
    assert_eq!(badge_url, format!("https://anc.dev/badge/{slug}.svg"));
    assert_eq!(scorecard_url, format!("https://anc.dev/score/{slug}"));

    // The two URL families MUST share the slug — a regression that
    // computed them from different sources would point readers at one
    // tool's scorecard via another tool's badge.
    assert!(badge_url.contains(slug));
    assert!(scorecard_url.contains(slug));
}

#[test]
fn schema_v05_badge_eligibility_flag_matches_score() {
    // Whatever the live score, `badge.eligible` must agree with
    // `score_pct >= 80`. A regression that flipped the comparison or
    // hard-coded `eligible: true` would slip the floor — caught here.
    let path = fixture_path("perfect-rust");
    let output = cmd()
        .args(["check", &path, "--output", "json"])
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: Value = serde_json::from_str(&stdout).expect("valid JSON");

    let pct = parsed["badge"]["score_pct"]
        .as_u64()
        .expect("score_pct is numeric") as u32;
    let eligible = parsed["badge"]["eligible"]
        .as_bool()
        .expect("eligible is boolean");
    assert_eq!(
        eligible,
        pct >= 80,
        "badge.eligible must equal (score_pct >= 80); got pct={pct}, eligible={eligible}",
    );

    // Embed snippet contract: present iff eligible. A non-eligible tool
    // emitting an embed would defeat the do-not-nag rule.
    if eligible {
        assert!(parsed["badge"]["embed_markdown"].is_string());
    } else {
        assert!(parsed["badge"]["embed_markdown"].is_null());
    }
}
