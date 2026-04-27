pub mod audience;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use serde::Serialize;

use crate::check::Check;
use crate::principles::registry::{Level, REQUIREMENTS, SPEC_VERSION};
use crate::types::{CheckGroup, CheckResult, CheckStatus};

/// Current scorecard JSON schema version. Consumers (site rendering,
/// leaderboard pipeline) pin against this to detect shape changes.
///
/// `0.x` is pre-launch — shape may still evolve. Will lock to `1.0` on
/// first public release of `anc`. During `0.x`, additive fields are the
/// norm; consumers feature-detect new keys rather than pinning exact
/// values. History: `0.1` (initial), `0.2` (audience, audit_profile,
/// coverage_summary), `0.3` (spec_version).
pub const SCHEMA_VERSION: &str = "0.3";

/// Pre-launch (`0.x`) scorecard shape emitted by `anc check --output json`.
///
/// **Scorecard-level enum values are kebab-case.** Both `audience` and
/// `audit_profile` serialize their enum values as kebab-case strings
/// (`agent-optimized` / `mixed` / `human-primary` for `audience`;
/// `human-tui` / `file-traversal` / `posix-utility` / `diagnostic-only`
/// for `audit_profile`). `audit_profile` MUST be kebab-case because it
/// echoes the CLI flag value a caller types (`--audit-profile human-tui`);
/// `audience` uses the same convention so consumers don't have to juggle
/// two casing rules inside one JSON document.
///
/// Per-result enum values in `results[].group` / `layer` / `confidence`
/// stay snake_case via their `#[serde(rename_all = "snake_case")]`
/// derives — they are a different contract (one row per check) with
/// broader consumer history, and share spelling with the Rust
/// type-system identifiers they come from.
///
/// Consumers key on the exact string; never transform case.
#[derive(Serialize)]
pub struct Scorecard {
    pub schema_version: &'static str,
    pub results: Vec<CheckResultView>,
    pub summary: Summary,
    pub coverage_summary: CoverageSummary,
    /// Derived audience classification (`agent-optimized`, `mixed`,
    /// `human-primary`). Reserved in `anc` v0.1.1 / v0.1.2 (always `null`);
    /// populated in v0.1.3+. Pre-launch additive (schema `0.2`); older
    /// consumers feature-detect.
    pub audience: Option<String>,
    /// When `audience` is `null`, the reason the classifier declined to
    /// label: `suppressed` (signal check masked by `--audit-profile`) or
    /// `insufficient_signal` (signal check never produced, e.g. source-only
    /// run). Omitted from JSON when `audience` has a label. Pre-launch
    /// additive (schema `0.2`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience_reason: Option<String>,
    /// Registry-sourced exemption category (human-tui, file-traversal, etc.).
    /// Reserved for `anc` v0.1.3; emitted as `null` in v0.1.1 / v0.1.2.
    /// Pre-launch additive (schema `0.2`).
    pub audit_profile: Option<String>,
    /// agentnative-spec version this CLI was built against. Sourced at build
    /// time from `src/principles/spec/VERSION` by `build.rs`. Reads
    /// `"unknown"` if the vendored VERSION file was missing at build time
    /// (build still succeeds; warning emitted). Pre-launch additive
    /// (schema `0.3`); older consumers feature-detect.
    pub spec_version: &'static str,
}

/// Per-level verification counts: how many requirements at this level had
/// at least one check in this run that declared `covers()` against them.
/// A requirement is "verified" regardless of pass/fail — the status tells
/// the consumer whether verification succeeded, this counter tells them
/// whether it was attempted at all.
#[derive(Serialize)]
pub struct LevelCounts {
    pub total: usize,
    pub verified: usize,
}

#[derive(Serialize)]
pub struct CoverageSummary {
    pub must: LevelCounts,
    pub should: LevelCounts,
    pub may: LevelCounts,
}

#[derive(Serialize)]
pub struct Summary {
    pub total: usize,
    pub pass: usize,
    pub warn: usize,
    pub fail: usize,
    pub skip: usize,
    pub error: usize,
}

#[derive(Serialize)]
pub struct CheckResultView {
    pub id: String,
    pub label: String,
    pub group: String,
    pub layer: String,
    pub status: String,
    pub evidence: Option<String>,
    /// `high` for direct probes, `medium` for heuristics. Older consumers
    /// feature-detect and tolerate missing keys.
    pub confidence: String,
}

impl CheckResultView {
    pub fn from_result(r: &CheckResult) -> Self {
        let (status, evidence) = match &r.status {
            CheckStatus::Pass => ("pass".to_string(), None),
            CheckStatus::Warn(e) => ("warn".to_string(), Some(e.clone())),
            CheckStatus::Fail(e) => ("fail".to_string(), Some(e.clone())),
            CheckStatus::Skip(e) => ("skip".to_string(), Some(e.clone())),
            CheckStatus::Error(e) => ("error".to_string(), Some(e.clone())),
        };
        // Serialize CheckGroup / CheckLayer / Confidence via serde_json so
        // the JSON mirrors the canonical enum spelling (snake_case).
        let group = serde_json::to_value(r.group)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| format!("{:?}", r.group));
        let layer = serde_json::to_value(r.layer)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| format!("{:?}", r.layer));
        let confidence = serde_json::to_value(r.confidence)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| format!("{:?}", r.confidence));
        CheckResultView {
            id: r.id.clone(),
            label: r.label.clone(),
            group,
            layer,
            status,
            evidence,
            confidence,
        }
    }
}

fn build_summary(results: &[CheckResult]) -> Summary {
    Summary {
        total: results.len(),
        pass: results
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::Pass))
            .count(),
        warn: results
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::Warn(_)))
            .count(),
        fail: results
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::Fail(_)))
            .count(),
        skip: results
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::Skip(_)))
            .count(),
        error: results
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::Error(_)))
            .count(),
    }
}

fn group_display(group: &CheckGroup) -> &'static str {
    match group {
        CheckGroup::P1 => "P1 — Non-Interactive by Default",
        CheckGroup::P2 => "P2 — Structured Output",
        CheckGroup::P3 => "P3 — Progressive Help",
        CheckGroup::P4 => "P4 — Actionable Errors",
        CheckGroup::P5 => "P5 — Safe Retries",
        CheckGroup::P6 => "P6 — Composable Structure",
        CheckGroup::P7 => "P7 — Bounded Responses",
        CheckGroup::CodeQuality => "Code Quality",
        CheckGroup::ProjectStructure => "Project Structure",
    }
}

/// Order groups for consistent display.
fn group_order(group: &CheckGroup) -> u8 {
    match group {
        CheckGroup::P1 => 1,
        CheckGroup::P2 => 2,
        CheckGroup::P3 => 3,
        CheckGroup::P4 => 4,
        CheckGroup::P5 => 5,
        CheckGroup::P6 => 6,
        CheckGroup::P7 => 7,
        CheckGroup::CodeQuality => 8,
        CheckGroup::ProjectStructure => 9,
    }
}

pub fn format_text(results: &[CheckResult], quiet: bool) -> String {
    let mut out = String::new();

    // Group results by CheckGroup
    let mut grouped: BTreeMap<u8, (CheckGroup, Vec<&CheckResult>)> = BTreeMap::new();
    for r in results {
        let order = group_order(&r.group);
        grouped
            .entry(order)
            .or_insert_with(|| (r.group, Vec::new()))
            .1
            .push(r);
    }

    for (group, checks) in grouped.values() {
        if !quiet {
            let _ = writeln!(out, "\n{}", group_display(group));
        }
        for r in checks {
            let prefix = match &r.status {
                CheckStatus::Pass => {
                    if quiet {
                        continue;
                    }
                    "PASS"
                }
                CheckStatus::Warn(_) => "WARN",
                CheckStatus::Fail(_) => "FAIL",
                CheckStatus::Skip(_) => {
                    if quiet {
                        continue;
                    }
                    "SKIP"
                }
                CheckStatus::Error(_) => "ERR ",
            };
            let _ = writeln!(out, "  [{prefix}] {} ({})", r.label, r.id);
            match &r.status {
                CheckStatus::Warn(e) | CheckStatus::Fail(e) | CheckStatus::Error(e) => {
                    for line in e.lines() {
                        let _ = writeln!(out, "         {line}");
                    }
                }
                CheckStatus::Skip(reason) if !quiet => {
                    let _ = writeln!(out, "         {reason}");
                }
                _ => {}
            }
        }
    }

    // Summary line
    let s = build_summary(results);
    let _ = writeln!(
        out,
        "\n{} checks: {} pass, {} warn, {} fail, {} skip, {} error",
        s.total, s.pass, s.warn, s.fail, s.skip, s.error
    );

    out
}

/// Build the scorecard. The `ran_checks` slice is the catalog of checks
/// that produced `results` — needed to translate check IDs back to the
/// requirement IDs they cover for `coverage_summary`.
pub fn build_scorecard(
    results: &[CheckResult],
    ran_checks: &[Box<dyn Check>],
    audience: Option<String>,
    audit_profile: Option<String>,
) -> Scorecard {
    // `audience_reason` is derived from `results` rather than threaded
    // through as a caller parameter — the reason is a property of the
    // result set, not a caller decision, and deriving it here keeps the
    // label and its explanation in lock-step. When audience has a label
    // the field is omitted from JSON (see Scorecard's serde skip rule).
    let audience_reason = if audience.is_some() {
        None
    } else {
        audience::classify_reason(results).map(|s| s.to_string())
    };
    Scorecard {
        schema_version: SCHEMA_VERSION,
        results: results.iter().map(CheckResultView::from_result).collect(),
        summary: build_summary(results),
        coverage_summary: build_coverage_summary(results, ran_checks),
        audience,
        audience_reason,
        audit_profile,
        spec_version: SPEC_VERSION,
    }
}

pub fn format_json(
    results: &[CheckResult],
    ran_checks: &[Box<dyn Check>],
    audience: Option<String>,
    audit_profile: Option<String>,
) -> String {
    let scorecard = build_scorecard(results, ran_checks, audience, audit_profile);
    serde_json::to_string_pretty(&scorecard).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
}

fn build_coverage_summary(
    results: &[CheckResult],
    ran_checks: &[Box<dyn Check>],
) -> CoverageSummary {
    // Map each ran check to its covers() so we can turn the set of ran
    // check IDs into a set of covered requirement IDs.
    let covers_by_id: HashMap<&str, &'static [&'static str]> =
        ran_checks.iter().map(|c| (c.id(), c.covers())).collect();

    // Verified = requirements covered by a check that actually executed.
    // A check suppressed by --audit-profile did NOT verify its
    // requirement — it emitted Skip with the `SUPPRESSION_EVIDENCE_PREFIX`
    // sentinel. Counting it toward `verified` would overstate coverage on
    // any --audit-profile run (a misleading public metric for the site
    // leaderboard). Filter those out here and mirror the exclusion in the
    // regression test below.
    let mut verified: HashSet<&'static str> = HashSet::new();
    for r in results {
        if audience::is_audit_profile_suppression(&r.status) {
            continue;
        }
        if let Some(ids) = covers_by_id.get(r.id.as_str()) {
            verified.extend(ids.iter().copied());
        }
    }

    let mut must = LevelCounts {
        total: 0,
        verified: 0,
    };
    let mut should = LevelCounts {
        total: 0,
        verified: 0,
    };
    let mut may = LevelCounts {
        total: 0,
        verified: 0,
    };

    for req in REQUIREMENTS {
        let bucket = match req.level {
            Level::Must => &mut must,
            Level::Should => &mut should,
            Level::May => &mut may,
        };
        bucket.total += 1;
        if verified.contains(req.id) {
            bucket.verified += 1;
        }
    }

    CoverageSummary { must, should, may }
}

/// Derive the process exit code from the full result set.
///
/// - `0` — every check Pass or Skip.
/// - `1` — at least one Warn.
/// - `2` — at least one Fail or Error.
///
/// **`--audit-profile` affects the exit code by masking Fails to Skips.**
/// A check that would otherwise Fail but is suppressed by the applied
/// profile contributes nothing to `has_fail_or_error` and cannot lift the
/// code above `0`/`1`. This is intentional per plan R4: the caller is
/// declaring "this category of check doesn't apply to this tool", so
/// scoring against that requirement would produce a misleading non-zero
/// exit. The tradeoff is that callers passing the wrong profile can
/// silently bless a broken tool — guarding against that lives upstream
/// (site's regen script, CI policy), not here.
pub fn exit_code(results: &[CheckResult]) -> i32 {
    let has_fail_or_error = results
        .iter()
        .any(|r| matches!(r.status, CheckStatus::Fail(_) | CheckStatus::Error(_)));
    let has_warn = results
        .iter()
        .any(|r| matches!(r.status, CheckStatus::Warn(_)));

    if has_fail_or_error {
        2
    } else if has_warn {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

    fn make_result(id: &str, status: CheckStatus, group: CheckGroup) -> CheckResult {
        CheckResult {
            id: id.to_string(),
            label: format!("Test {id}"),
            group,
            layer: CheckLayer::Behavioral,
            status,
            confidence: Confidence::High,
        }
    }

    #[test]
    fn test_format_json_valid() {
        let results = vec![
            make_result("c1", CheckStatus::Pass, CheckGroup::P1),
            make_result("c2", CheckStatus::Fail("bad".into()), CheckGroup::P2),
        ];
        let json = format_json(&results, &[], None, None);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["schema_version"], "0.3");
        assert_eq!(parsed["summary"]["total"], 2);
        assert_eq!(parsed["summary"]["pass"], 1);
        assert_eq!(parsed["summary"]["fail"], 1);
        assert_eq!(parsed["results"][0]["status"], "pass");
        assert!(parsed["results"][0]["evidence"].is_null());
        assert_eq!(parsed["results"][0]["confidence"], "high");
        assert_eq!(parsed["results"][1]["status"], "fail");
        assert_eq!(parsed["results"][1]["evidence"], "bad");
        assert_eq!(parsed["results"][1]["confidence"], "high");
        // 0.2 additions: coverage_summary present with three levels, audience + audit_profile null.
        assert!(parsed["coverage_summary"]["must"]["total"].is_number());
        assert!(parsed["coverage_summary"]["should"]["total"].is_number());
        assert!(parsed["coverage_summary"]["may"]["total"].is_number());
        assert!(parsed["audience"].is_null());
        assert!(parsed["audit_profile"].is_null());
        // 0.3 addition: spec_version is always present and non-empty.
        let spec = parsed["spec_version"]
            .as_str()
            .expect("spec_version is a string");
        assert!(!spec.is_empty(), "spec_version must not be empty");
    }

    #[test]
    fn medium_confidence_serializes_as_medium() {
        let mut r = make_result("c3", CheckStatus::Warn("soft".into()), CheckGroup::P6);
        r.confidence = Confidence::Medium;
        let view = CheckResultView::from_result(&r);
        assert_eq!(view.confidence, "medium");
    }

    #[test]
    fn coverage_summary_counts_verified_requirements() {
        use crate::check::Check;
        use crate::project::Project;
        use crate::types::CheckLayer;

        struct FakeCheck {
            id: &'static str,
            covers: &'static [&'static str],
        }

        impl Check for FakeCheck {
            fn id(&self) -> &str {
                self.id
            }
            fn label(&self) -> &'static str {
                "fake"
            }
            fn group(&self) -> CheckGroup {
                CheckGroup::P1
            }
            fn layer(&self) -> CheckLayer {
                CheckLayer::Behavioral
            }
            fn applicable(&self, _p: &Project) -> bool {
                true
            }
            fn run(&self, _p: &Project) -> anyhow::Result<CheckResult> {
                unreachable!()
            }
            fn covers(&self) -> &'static [&'static str] {
                self.covers
            }
        }

        let results = vec![make_result("verifier-a", CheckStatus::Pass, CheckGroup::P1)];
        let checks: Vec<Box<dyn Check>> = vec![Box::new(FakeCheck {
            id: "verifier-a",
            covers: &["p1-must-no-interactive"],
        })];

        let summary = build_coverage_summary(&results, &checks);
        assert_eq!(summary.must.verified, 1);
        assert_eq!(summary.should.verified, 0);
        assert_eq!(summary.may.verified, 0);
        // Totals match the registry snapshot baked into registry.rs tests.
        assert!(summary.must.total >= 1);
    }

    #[test]
    fn coverage_summary_excludes_audit_profile_suppressed_checks() {
        use crate::check::Check;
        use crate::principles::registry::SUPPRESSION_EVIDENCE_PREFIX;
        use crate::project::Project;
        use crate::types::CheckLayer;

        struct FakeCheck {
            id: &'static str,
            covers: &'static [&'static str],
        }

        impl Check for FakeCheck {
            fn id(&self) -> &str {
                self.id
            }
            fn label(&self) -> &'static str {
                "fake"
            }
            fn group(&self) -> CheckGroup {
                CheckGroup::P1
            }
            fn layer(&self) -> CheckLayer {
                CheckLayer::Behavioral
            }
            fn applicable(&self, _p: &Project) -> bool {
                true
            }
            fn run(&self, _p: &Project) -> anyhow::Result<CheckResult> {
                unreachable!()
            }
            fn covers(&self) -> &'static [&'static str] {
                self.covers
            }
        }

        // Two checks: one ran (Pass → counts as verified), one was
        // suppressed by --audit-profile (Skip with the sentinel prefix →
        // MUST NOT count as verified).
        let results = vec![
            make_result("verifier-ran", CheckStatus::Pass, CheckGroup::P1),
            make_result(
                "verifier-suppressed",
                CheckStatus::Skip(format!("{SUPPRESSION_EVIDENCE_PREFIX}human-tui")),
                CheckGroup::P1,
            ),
        ];
        let checks: Vec<Box<dyn Check>> = vec![
            Box::new(FakeCheck {
                id: "verifier-ran",
                covers: &["p1-must-no-interactive"],
            }),
            Box::new(FakeCheck {
                id: "verifier-suppressed",
                covers: &["p1-should-tty-detection"],
            }),
        ];

        let summary = build_coverage_summary(&results, &checks);
        assert_eq!(
            summary.must.verified, 1,
            "only the non-suppressed verifier's requirement should count; \
             suppressed Skips MUST NOT inflate coverage_summary.verified",
        );
        assert_eq!(summary.should.verified, 0);
    }

    #[test]
    fn test_exit_code_all_pass() {
        let results = vec![
            make_result("c1", CheckStatus::Pass, CheckGroup::P1),
            make_result("c2", CheckStatus::Skip("n/a".into()), CheckGroup::P2),
        ];
        assert_eq!(exit_code(&results), 0);
    }

    #[test]
    fn test_exit_code_warn() {
        let results = vec![
            make_result("c1", CheckStatus::Pass, CheckGroup::P1),
            make_result("c2", CheckStatus::Warn("meh".into()), CheckGroup::P2),
        ];
        assert_eq!(exit_code(&results), 1);
    }

    #[test]
    fn test_exit_code_fail() {
        let results = vec![
            make_result("c1", CheckStatus::Fail("bad".into()), CheckGroup::P1),
            make_result("c2", CheckStatus::Warn("meh".into()), CheckGroup::P2),
        ];
        assert_eq!(exit_code(&results), 2);
    }

    #[test]
    fn test_exit_code_error() {
        let results = vec![make_result(
            "c1",
            CheckStatus::Error("boom".into()),
            CheckGroup::P1,
        )];
        assert_eq!(exit_code(&results), 2);
    }

    #[test]
    fn test_check_result_view_conversion() {
        let r = make_result(
            "test-id",
            CheckStatus::Warn("warning msg".into()),
            CheckGroup::P3,
        );
        let view = CheckResultView::from_result(&r);
        assert_eq!(view.id, "test-id");
        assert_eq!(view.status, "warn");
        assert_eq!(view.evidence.as_deref(), Some("warning msg"));
        assert_eq!(view.layer, "behavioral");
    }

    #[test]
    fn test_check_result_view_pass_has_no_evidence() {
        let r = make_result("pass-id", CheckStatus::Pass, CheckGroup::P1);
        let view = CheckResultView::from_result(&r);
        assert_eq!(view.status, "pass");
        assert!(view.evidence.is_none());
    }

    #[test]
    fn format_json_emits_audience_when_all_signals_present() {
        use crate::scorecard::audience::{SIGNAL_CHECK_IDS, classify};

        let results: Vec<CheckResult> = SIGNAL_CHECK_IDS
            .iter()
            .map(|id| make_result(id, CheckStatus::Pass, CheckGroup::P1))
            .collect();
        let audience = classify(&results);
        let json = format_json(&results, &[], audience, None);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["audience"], "agent-optimized");
        assert!(parsed["audit_profile"].is_null());
        assert_eq!(parsed["schema_version"], "0.3");
    }

    #[test]
    fn format_json_emits_human_primary_when_signals_warn() {
        use crate::scorecard::audience::{SIGNAL_CHECK_IDS, classify};

        let results: Vec<CheckResult> = SIGNAL_CHECK_IDS
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let status = if i < 3 {
                    CheckStatus::Warn(format!("missing {id}"))
                } else {
                    CheckStatus::Pass
                };
                make_result(id, status, CheckGroup::P1)
            })
            .collect();
        let audience = classify(&results);
        let json = format_json(&results, &[], audience, None);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["audience"], "human-primary");
    }

    #[test]
    fn format_json_audience_null_when_signals_missing() {
        use crate::scorecard::audience::classify;

        // Source-only-style run: no behavioral checks, so no signal IDs.
        let results = vec![make_result(
            "p1-env-flags-source",
            CheckStatus::Pass,
            CheckGroup::P1,
        )];
        let audience = classify(&results);
        let json = format_json(&results, &[], audience, None);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(parsed["audience"].is_null());
    }

    #[test]
    fn format_json_echoes_audit_profile() {
        let results = vec![make_result("c1", CheckStatus::Pass, CheckGroup::P1)];
        let json = format_json(&results, &[], None, Some("human-tui".into()));
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["audit_profile"], "human-tui");
    }

    #[test]
    fn format_json_audience_reason_insufficient_signal() {
        // Source-only-style run: no signal checks → audience null and
        // audience_reason must explain why.
        let results = vec![make_result(
            "p1-env-flags-source",
            CheckStatus::Pass,
            CheckGroup::P1,
        )];
        let json = format_json(&results, &[], None, None);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(parsed["audience"].is_null());
        assert_eq!(parsed["audience_reason"], "insufficient_signal");
    }

    #[test]
    fn format_json_audience_reason_omitted_when_audience_labeled() {
        use crate::scorecard::audience::{SIGNAL_CHECK_IDS, classify};

        let results: Vec<CheckResult> = SIGNAL_CHECK_IDS
            .iter()
            .map(|id| make_result(id, CheckStatus::Pass, CheckGroup::P1))
            .collect();
        let audience = classify(&results);
        let json = format_json(&results, &[], audience, None);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        // audience has a label, so audience_reason must be omitted — not
        // merely null. `#[serde(skip_serializing_if = "Option::is_none")]`
        // on the field makes this verifiable by key presence.
        assert_eq!(parsed["audience"], "agent-optimized");
        assert!(
            parsed.get("audience_reason").is_none(),
            "audience_reason key should be absent when audience is labeled, got {}",
            parsed["audience_reason"],
        );
    }

    #[test]
    fn format_json_audience_reason_suppressed() {
        use crate::principles::registry::SUPPRESSION_EVIDENCE_PREFIX;
        use crate::scorecard::audience::{SIGNAL_CHECK_IDS, classify};

        // One signal suppressed → audience null and reason "suppressed".
        let results: Vec<CheckResult> = SIGNAL_CHECK_IDS
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let status = if i == 0 {
                    CheckStatus::Skip(format!("{SUPPRESSION_EVIDENCE_PREFIX}human-tui"))
                } else {
                    CheckStatus::Pass
                };
                make_result(id, status, CheckGroup::P1)
            })
            .collect();
        let audience = classify(&results);
        let json = format_json(&results, &[], audience, Some("human-tui".into()));
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert!(parsed["audience"].is_null());
        assert_eq!(parsed["audience_reason"], "suppressed");
    }

    #[test]
    fn exit_code_drops_when_audit_profile_suppresses_a_would_have_failed_check() {
        // Intentional behavior per plan R4: when --audit-profile suppresses
        // a check that would otherwise Fail, the check emits Skip with the
        // suppression prefix and the overall exit code reflects the
        // masked state. This is a trust-boundary choice — the caller
        // declared the requirement doesn't apply, so failing on it would
        // be misleading.
        //
        // This test pins the behavior against a future well-meaning
        // change that tries to "refuse to exit 0 if any check was
        // suppressed." Such a change must update this test deliberately
        // and resolve the conflict with plan R4, not sneak through.
        use crate::principles::registry::SUPPRESSION_EVIDENCE_PREFIX;

        let baseline = vec![
            make_result("c-pass", CheckStatus::Pass, CheckGroup::P1),
            make_result(
                "c-would-fail",
                CheckStatus::Fail("violates MUST".into()),
                CheckGroup::P1,
            ),
        ];
        assert_eq!(exit_code(&baseline), 2, "baseline: a Fail → exit 2");

        let suppressed = vec![
            make_result("c-pass", CheckStatus::Pass, CheckGroup::P1),
            make_result(
                "c-would-fail",
                CheckStatus::Skip(format!("{SUPPRESSION_EVIDENCE_PREFIX}human-tui")),
                CheckGroup::P1,
            ),
        ];
        assert_eq!(
            exit_code(&suppressed),
            0,
            "suppression by audit_profile must lower the exit code — \
             Fail → Skip is intentional masking per plan R4",
        );
    }

    #[test]
    fn scorecard_level_enum_values_are_kebab_case() {
        // Both `audience` and `audit_profile` enum values MUST serialize
        // as kebab-case inside the scorecard JSON. `audit_profile`
        // echoes the CLI flag value (`--audit-profile human-tui`) and
        // cannot change casing; `audience` adopts the same convention so
        // consumers don't juggle two rules inside one document.
        //
        // A future serde `rename_all` edit, field reorder, or enum
        // migration that silently flips either convention must fail here
        // loudly. The snake_case negative assertions below guard against
        // the most likely regression direction (adopting the per-result
        // enum convention from `CheckGroup` / `CheckLayer` / `Confidence`).
        use crate::scorecard::audience::{SIGNAL_CHECK_IDS, classify};

        let results: Vec<CheckResult> = SIGNAL_CHECK_IDS
            .iter()
            .map(|id| make_result(id, CheckStatus::Pass, CheckGroup::P1))
            .collect();
        let audience = classify(&results);
        let json = format_json(&results, &[], audience, Some("human-tui".into()));

        // audience: kebab-case.
        assert!(
            json.contains("\"audience\": \"agent-optimized\""),
            "audience must serialize as kebab-case 'agent-optimized', got:\n{json}",
        );
        assert!(
            !json.contains("\"agent_optimized\""),
            "audience must NOT render as snake_case 'agent_optimized' — \
             kebab-case unified with audit_profile in v0.1.3",
        );
        assert!(
            !json.contains("\"human_primary\""),
            "audience must NOT render as snake_case 'human_primary'",
        );

        // audit_profile: kebab-case (echo of the CLI flag value).
        assert!(
            json.contains("\"audit_profile\": \"human-tui\""),
            "audit_profile must serialize as kebab-case 'human-tui', got:\n{json}",
        );
        assert!(
            !json.contains("\"human_tui\""),
            "audit_profile must NOT render as snake_case 'human_tui' — \
             would desync from the --audit-profile flag value shape",
        );
    }
}
