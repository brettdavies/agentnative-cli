use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;

use crate::types::{CheckGroup, CheckResult, CheckStatus};

#[derive(Serialize)]
pub struct Scorecard {
    pub results: Vec<CheckResultView>,
    pub summary: Summary,
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
        // Serialize CheckGroup via serde_json for canonical format
        let group = serde_json::to_value(&r.group)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| format!("{:?}", r.group));
        let layer = serde_json::to_value(&r.layer)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| format!("{:?}", r.layer));
        CheckResultView {
            id: r.id.clone(),
            label: r.label.clone(),
            group,
            layer,
            status,
            evidence,
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

    for (_order, (group, checks)) in &grouped {
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

pub fn format_json(results: &[CheckResult]) -> String {
    let scorecard = Scorecard {
        results: results.iter().map(CheckResultView::from_result).collect(),
        summary: build_summary(results),
    };
    // serde_json::to_string_pretty should not fail on this struct
    serde_json::to_string_pretty(&scorecard).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
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
    use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

    fn make_result(id: &str, status: CheckStatus, group: CheckGroup) -> CheckResult {
        CheckResult {
            id: id.to_string(),
            label: format!("Test {id}"),
            group,
            layer: CheckLayer::Behavioral,
            status,
        }
    }

    #[test]
    fn test_format_json_valid() {
        let results = vec![
            make_result("c1", CheckStatus::Pass, CheckGroup::P1),
            make_result("c2", CheckStatus::Fail("bad".into()), CheckGroup::P2),
        ];
        let json = format_json(&results);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["summary"]["total"], 2);
        assert_eq!(parsed["summary"]["pass"], 1);
        assert_eq!(parsed["summary"]["fail"], 1);
        assert_eq!(parsed["results"][0]["status"], "pass");
        assert!(parsed["results"][0]["evidence"].is_null());
        assert_eq!(parsed["results"][1]["status"], "fail");
        assert_eq!(parsed["results"][1]["evidence"], "bad");
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
