use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use serde::Serialize;

use crate::check::Check;
use crate::principles::registry::{Level, REQUIREMENTS};
use crate::types::{CheckGroup, CheckResult, CheckStatus};

/// Current scorecard JSON schema version. Consumers (site rendering,
/// leaderboard pipeline) pin against this to detect shape changes.
pub const SCHEMA_VERSION: &str = "1.1";

#[derive(Serialize)]
pub struct Scorecard {
    pub schema_version: &'static str,
    pub results: Vec<CheckResultView>,
    pub summary: Summary,
    pub coverage_summary: CoverageSummary,
    /// Derived audience classification (human-primary, agent-primary, mixed).
    /// Reserved for v0.1.3; emitted as `null` in v0.1.1 / v0.1.2.
    pub audience: Option<String>,
    /// Registry-sourced exemption category (human-tui, file-traversal, etc.).
    /// Reserved for v0.1.3; emitted as `null` in v0.1.1 / v0.1.2.
    pub audit_profile: Option<String>,
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
    /// `high` for direct probes, `medium` for heuristics. Additive field;
    /// v1.1 consumers feature-detect and tolerate missing keys.
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

/// Build a v1.1 scorecard. The `ran_checks` slice is the catalog of checks
/// that produced `results` — needed to translate check IDs back to the
/// requirement IDs they cover for `coverage_summary`.
pub fn build_scorecard(
    results: &[CheckResult],
    ran_checks: &[Box<dyn Check>],
    audience: Option<String>,
    audit_profile: Option<String>,
) -> Scorecard {
    Scorecard {
        schema_version: SCHEMA_VERSION,
        results: results.iter().map(CheckResultView::from_result).collect(),
        summary: build_summary(results),
        coverage_summary: build_coverage_summary(results, ran_checks),
        audience,
        audit_profile,
    }
}

pub fn format_json(results: &[CheckResult], ran_checks: &[Box<dyn Check>]) -> String {
    let scorecard = build_scorecard(results, ran_checks, None, None);
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

    let mut verified: HashSet<&'static str> = HashSet::new();
    for r in results {
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
        let json = format_json(&results, &[]);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["schema_version"], "1.1");
        assert_eq!(parsed["summary"]["total"], 2);
        assert_eq!(parsed["summary"]["pass"], 1);
        assert_eq!(parsed["summary"]["fail"], 1);
        assert_eq!(parsed["results"][0]["status"], "pass");
        assert!(parsed["results"][0]["evidence"].is_null());
        assert_eq!(parsed["results"][0]["confidence"], "high");
        assert_eq!(parsed["results"][1]["status"], "fail");
        assert_eq!(parsed["results"][1]["evidence"], "bad");
        assert_eq!(parsed["results"][1]["confidence"], "high");
        // v1.1 additions: coverage_summary present with three levels, audience + audit_profile null.
        assert!(parsed["coverage_summary"]["must"]["total"].is_number());
        assert!(parsed["coverage_summary"]["should"]["total"].is_number());
        assert!(parsed["coverage_summary"]["may"]["total"].is_number());
        assert!(parsed["audience"].is_null());
        assert!(parsed["audit_profile"].is_null());
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
}
