//! End-to-end schema 0.5 drift guard.
//!
//! Spawns the real binary in each of the three `anc audit` modes (project,
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

/// Assert every documented v0.5 key path resolves on the parsed JSON, plus
/// the v0.7 additions (per-row emission, `tier` + `audit_id` on each
/// result, `opt_out` / `n_a` summary counters). The segmented walk gives a
/// precise failure message when a field is missing.
fn assert_v05_shape(parsed: &Value) {
    assert_eq!(
        parsed["schema_version"], "0.7",
        "schema_version must be 0.7 (per-row emission + 7-status taxonomy)",
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
        // 0.7 additions — 7-status summary counters.
        "summary.opt_out",
        "summary.n_a",
    ] {
        let mut node = parsed;
        for segment in path.split('.') {
            node = node
                .get(segment)
                .unwrap_or_else(|| panic!("expected key `{path}` — segment `{segment}` missing"));
        }
    }

    // 0.7: every result row carries `tier` and `audit_id`. The shape is
    // assertable as soon as `results[]` is non-empty.
    if let Some(results) = parsed["results"].as_array() {
        for (i, row) in results.iter().enumerate() {
            assert!(
                row.get("tier").is_some(),
                "results[{i}] missing `tier` (schema 0.7): {row}",
            );
            assert!(
                row.get("audit_id").is_some(),
                "results[{i}] missing `audit_id` (schema 0.7): {row}",
            );
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
        .args(["audit", &path, "--output", "json"])
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: Value = serde_json::from_str(&stdout).expect("valid JSON");

    assert_v05_shape(&parsed);
    assert_eq!(parsed["target"]["kind"], "project");
    assert_eq!(
        parsed["target"]["path"], "perfect-rust",
        "project mode emits the basename of the resolved target, not the absolute path \
         (PII-leak guard — operator home dir / org dir structure must not appear)",
    );
    assert!(parsed["target"]["command"].is_null());
}

#[test]
fn schema_v05_binary_mode_emits_full_shape() {
    let path = fixture_path("binary-only/test.sh");
    let output = cmd()
        .args(["audit", &path, "--output", "json"])
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: Value = serde_json::from_str(&stdout).expect("valid JSON");

    assert_v05_shape(&parsed);
    assert_eq!(parsed["target"]["kind"], "binary");
    assert_eq!(
        parsed["target"]["path"], "test.sh",
        "binary mode emits the basename of the resolved target, not the absolute path \
         (PII-leak guard)",
    );
    assert!(parsed["target"]["command"].is_null());
}

/// Regression guard — `target.path` must never contain a path separator. If
/// this trips, the absolute-path leak from pre-fix `build_target_info` has
/// crept back in (or someone built a new code path that emits a richer
/// path representation). See `src/main.rs::build_target_info` doc comment
/// for the leak-vector rationale.
#[test]
fn schema_v05_target_path_carries_no_separators() {
    let path = fixture_path("perfect-rust");
    let output = cmd()
        .args(["audit", &path, "--output", "json"])
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: Value = serde_json::from_str(&stdout).expect("valid JSON");

    let target_path = parsed["target"]["path"]
        .as_str()
        .expect("project mode emits a string target.path");
    assert!(
        !target_path.contains('/') && !target_path.contains('\\'),
        "target.path must be a basename only, got: {target_path:?}",
    );
}

#[test]
fn schema_v05_command_mode_emits_full_shape() {
    // `echo` exists on every supported platform; the version probe is
    // best-effort and tolerates whatever `echo --version` happens to print.
    let output = cmd()
        .args(["audit", "--command", "echo", "--output", "json"])
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
    // must see `anc <path>` in the scorecard, NOT `anc audit <path>`.
    let path = fixture_path("perfect-rust");
    let output = cmd()
        .args([&path, "--output", "json"]) // no explicit `audit`
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let parsed: Value = serde_json::from_str(&stdout).expect("valid JSON");

    let invocation = parsed["run"]["invocation"]
        .as_str()
        .expect("run.invocation is a string");
    assert!(
        !invocation.contains(" audit "),
        "run.invocation must reflect user intent (pre-injection), got: {invocation}",
    );
}

#[test]
fn schema_v05_run_platform_matches_runtime_os_arch() {
    let path = fixture_path("perfect-rust");
    let output = cmd()
        .args(["audit", &path, "--output", "json"])
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
        .args(["audit", &path, "--output", "json"])
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
        .args(["audit", &path, "--output", "json"])
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
    // audits evolve, so we only assert URL shape, not eligibility).
    let path = fixture_path("perfect-rust");
    let output = cmd()
        .args(["audit", &path, "--output", "json"])
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
        .args(["audit", &path, "--output", "json"])
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

// ─────────────────────────────────────────────────────────────────────────
// Schema 0.7 red team: the committed scorecard.schema.json is the consumer
// contract for the site renderer and third-party leaderboards. A drift
// between the hand-written schema and the serde-derived live JSON would
// silently break those consumers. The tests below pin the shape contract
// from both directions.
// ─────────────────────────────────────────────────────────────────────────

/// Read the committed schema once. Returns the parsed JSON value so each
/// test can assert against a specific shape concern in isolation.
fn schema_doc() -> Value {
    let path = format!(
        "{}/schema/scorecard.schema.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).expect("schema file readable");
    serde_json::from_str(&text).expect("schema file is valid JSON")
}

#[test]
fn rt_schema_id_pins_to_published_schema_version() {
    // The schema's `$id` must match the SCHEMA_VERSION constant emitted by
    // the runtime. A bump that updates one without the other ships a
    // consumer contract that disagrees with itself.
    let schema = schema_doc();
    let id = schema["$id"].as_str().expect("$id is a string");
    assert!(
        id.contains("scorecard-v0.7"),
        "schema $id must pin to the current SCHEMA_VERSION (0.7), got: {id}",
    );
}

#[test]
fn rt_schema_status_enum_lists_all_seven_taxonomy_values() {
    // The 7-status taxonomy is the load-bearing contract of schema 0.7. A
    // drift here (missing `opt_out`, missing `n_a`, stray pre-0.7 value
    // dropped, etc.) would either silently mute new statuses on the
    // consumer side or fail validation against legitimate scorecards.
    let schema = schema_doc();
    let enums = schema["$defs"]["AuditResultView"]["properties"]["status"]["enum"]
        .as_array()
        .expect("status.enum is an array");
    let values: Vec<&str> = enums.iter().filter_map(|v| v.as_str()).collect();
    for expected in ["pass", "warn", "fail", "opt_out", "n_a", "skip", "error"] {
        assert!(
            values.contains(&expected),
            "status.enum missing `{expected}` — schema 0.7 contract violated. got: {values:?}",
        );
    }
    assert_eq!(
        values.len(),
        7,
        "status.enum must list exactly seven values; got {values:?}",
    );
}

#[test]
fn rt_schema_summary_required_includes_opt_out_and_n_a() {
    // Summary counters are an additive shape change: adding to `properties`
    // without adding to `required` would let consumers omit them silently.
    let schema = schema_doc();
    let required = schema["$defs"]["Summary"]["required"]
        .as_array()
        .expect("Summary.required is an array");
    let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
    for expected in [
        "total", "pass", "warn", "fail", "opt_out", "n_a", "skip", "error",
    ] {
        assert!(
            names.contains(&expected),
            "Summary.required missing `{expected}` — got: {names:?}",
        );
    }
}

#[test]
fn rt_schema_audit_result_view_includes_tier_and_audit_id() {
    // Schema 0.7 added `tier` and `audit_id` to every results[] entry.
    // The drift guard pins both presence and the tier enum's three values
    // (plus null for unknown row ids).
    let schema = schema_doc();
    let props = &schema["$defs"]["AuditResultView"]["properties"];
    assert!(props["tier"].is_object(), "AuditResultView.tier missing");
    assert!(
        props["audit_id"].is_object(),
        "AuditResultView.audit_id missing",
    );
    let tier_enum = props["tier"]["enum"]
        .as_array()
        .expect("tier.enum is array");
    let tier_values: Vec<&str> = tier_enum.iter().filter_map(|v| v.as_str()).collect();
    for expected in ["must", "should", "may"] {
        assert!(
            tier_values.contains(&expected),
            "tier.enum missing `{expected}`, got: {tier_values:?}",
        );
    }
    // Also accepts null for rows whose id is not in the registry.
    assert!(
        tier_enum.iter().any(|v| v.is_null()),
        "tier.enum must permit null for unknown row ids, got: {tier_enum:?}",
    );
}

#[test]
fn rt_schema_example_block_passes_its_own_required_keys() {
    // The schema's `examples[0]` is documentation surface — if it drifts
    // from the actual `required` lists, agents copying it as a template
    // will produce invalid scorecards. Walk the required[] tree and assert
    // every key resolves on the example.
    let schema = schema_doc();
    let example = &schema["examples"][0];
    assert!(example.is_object(), "examples[0] must be an object");

    let top_required = schema["required"]
        .as_array()
        .expect("top-level required is array");
    for key_val in top_required {
        let key = key_val.as_str().expect("required entry is string");
        assert!(
            example.get(key).is_some(),
            "examples[0] missing top-level required key `{key}`",
        );
    }

    // Walk into results[0] and assert its required keys are present too.
    let result_example = &example["results"][0];
    let result_required = schema["$defs"]["AuditResultView"]["required"]
        .as_array()
        .expect("AuditResultView.required is array");
    for key_val in result_required {
        let key = key_val.as_str().expect("required entry is string");
        assert!(
            result_example.get(key).is_some(),
            "examples[0].results[0] missing required key `{key}`",
        );
    }

    // And the summary block.
    let summary_example = &example["summary"];
    let summary_required = schema["$defs"]["Summary"]["required"]
        .as_array()
        .expect("Summary.required is array");
    for key_val in summary_required {
        let key = key_val.as_str().expect("required entry is string");
        assert!(
            summary_example.get(key).is_some(),
            "examples[0].summary missing required key `{key}`",
        );
    }
}

#[test]
fn rt_live_scorecard_top_level_keys_match_schema_required() {
    // The strongest drift guard: spawn the real binary, produce a live
    // scorecard, and assert every key in the schema's top-level `required`
    // list is present. Any field added to the struct without a matching
    // schema entry, or removed from the schema without removing from the
    // struct, surfaces here.
    let path = fixture_path("perfect-rust");
    let output = cmd()
        .args(["audit", &path, "--output", "json"])
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let live: Value = serde_json::from_str(&stdout).expect("live JSON parses");

    let schema = schema_doc();
    let required = schema["required"]
        .as_array()
        .expect("top-level required is array");
    for key_val in required {
        let key = key_val.as_str().expect("required entry is string");
        assert!(
            live.get(key).is_some(),
            "live scorecard missing required top-level key `{key}` — \
             schema declares it but the live JSON omits it.",
        );
    }
}

#[test]
fn rt_live_results_rows_satisfy_audit_result_view_required_keys() {
    // Per-row contract: every row in results[] carries the keys declared
    // required by AuditResultView. Catches a probe that hand-builds a
    // AuditResult skipping a field, or a schema that lists a key the
    // serializer dropped.
    let path = fixture_path("perfect-rust");
    let output = cmd()
        .args(["audit", &path, "--output", "json"])
        .output()
        .expect("anc spawn");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let live: Value = serde_json::from_str(&stdout).expect("live JSON parses");

    let schema = schema_doc();
    let required: Vec<String> = schema["$defs"]["AuditResultView"]["required"]
        .as_array()
        .expect("AuditResultView.required is array")
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    let rows = live["results"].as_array().expect("results is array");
    assert!(!rows.is_empty(), "live run produced no rows");
    for (i, row) in rows.iter().enumerate() {
        for key in &required {
            assert!(
                row.get(key).is_some(),
                "results[{i}] missing required key `{key}`: row = {row}",
            );
        }
    }
}
