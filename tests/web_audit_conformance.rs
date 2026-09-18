//! The parity claim, as CI evidence: every vendored corpus scenario
//! replays through the Rust engine and its scorecard must serialize byte
//! for byte to the JSON the anc.dev engine wrote for the same exchanges.
//!
//! The corpus is the site's own recording at the single-hop fetch seam
//! (`tests/fixtures/web-audit-conformance/README.md`), so a divergence
//! here is a divergence in a verdict, an evidence line, a score, or a key
//! order, not in a test double. A scenario the Rust engine cannot yet
//! reproduce is named in `KNOWN_DIVERGENCES` with its reason; the list is
//! asserted empty-or-named, never silently skipped.

mod common;

use std::collections::BTreeMap;
use std::fs;

use agentnative::web_audit::engine::RunOutcome;
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::scorecard::to_wire_json;
use common::corpus::{Case, Golden, load_cases, replay};
use std::sync::Arc;

/// Scenarios whose divergence is understood and accepted, each with the
/// reason it cannot close. Empty means the port reproduces the whole
/// corpus; a non-empty entry is a documented gap, never a silent pass.
const KNOWN_DIVERGENCES: &[(&str, &str)] = &[];

fn known(name: &str) -> Option<&'static str> {
    KNOWN_DIVERGENCES
        .iter()
        .find(|(scenario, _)| *scenario == name)
        .map(|(_, reason)| *reason)
}

/// The first differing line of two JSON documents, with its neighbours, so
/// a failure names the field rather than dumping two scorecards.
fn first_difference(expected: &str, actual: &str) -> String {
    let (mut want, mut got) = (expected.lines(), actual.lines());
    let mut line = 0usize;
    loop {
        line += 1;
        match (want.next(), got.next()) {
            (None, None) => return "no textual difference (trailing bytes differ)".to_string(),
            (a, b) if a == b => continue,
            (a, b) => {
                return format!(
                    "line {line}:\n    expected {}\n    actual   {}",
                    a.unwrap_or("<end of file>").trim(),
                    b.unwrap_or("<end of file>").trim()
                );
            }
        }
    }
}

fn scorecard_json(case: &Case) -> Result<String, String> {
    match replay(&case.scenario, Arc::new(default_handlers())) {
        RunOutcome::Complete(report) => Ok(to_wire_json(&report.scorecard)),
        RunOutcome::Unreachable { reason, .. } => {
            Err(format!("the run reported unreachable: {reason}"))
        }
    }
}

fn golden_text(name: &str) -> String {
    let path = common::corpus::corpus_dir()
        .join("scenarios")
        .join(name)
        .join("scorecard.json");
    fs::read_to_string(path).expect("a golden scorecard")
}

#[test]
fn every_corpus_scenario_reproduces_the_site_scorecard_byte_for_byte() {
    let cases = load_cases();
    assert!(
        cases.len() >= 94,
        "the corpus shrank: {} scenarios",
        cases.len()
    );
    let mut failures: Vec<String> = Vec::new();
    let mut compared = 0usize;
    let mut unreachable = 0usize;

    for case in &cases {
        match &case.golden {
            Golden::Unreachable(expected) => {
                unreachable += 1;
                match replay(&case.scenario, Arc::new(default_handlers())) {
                    RunOutcome::Unreachable { reason, .. } if &reason == expected => {}
                    RunOutcome::Unreachable { reason, .. } => failures.push(format!(
                        "{}: unreachable reason differs\n    expected {expected:?}\n    actual   {reason:?}",
                        case.name
                    )),
                    RunOutcome::Complete(_) => failures.push(format!(
                        "{}: scored where the site reported unreachable",
                        case.name
                    )),
                }
            }
            Golden::Scorecard(_) => {
                compared += 1;
                let expected = golden_text(&case.name);
                match scorecard_json(case) {
                    Ok(actual) if actual == expected => {
                        if let Some(reason) = known(&case.name) {
                            failures.push(format!(
                                "{}: listed in KNOWN_DIVERGENCES ({reason}) but now matches; remove the entry",
                                case.name
                            ));
                        }
                    }
                    Ok(actual) => {
                        if known(&case.name).is_none() {
                            failures.push(format!(
                                "{}: scorecard differs at {}",
                                case.name,
                                first_difference(&expected, &actual)
                            ));
                        }
                    }
                    Err(problem) => failures.push(format!("{}: {problem}", case.name)),
                }
            }
        }
    }

    assert!(compared >= 90, "only {compared} scorecards compared");
    assert_eq!(unreachable, 2, "the corpus carries two unreachable goldens");
    assert!(
        failures.is_empty(),
        "{} of {} scenario(s) diverge from anc.dev:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

/// The fetch-layer semantics the port had to reproduce rather than inherit
/// each get their own named scenario, so a regression in one of them is a
/// named failure instead of a number moving in a whole-corpus diff.
#[test]
fn the_fetch_parity_scenarios_are_present_and_green() {
    let by_name: BTreeMap<String, Case> = load_cases()
        .into_iter()
        .map(|case| (case.name.clone(), case))
        .collect();
    let semantics = [
        (
            "run-redirects",
            "a redirect chain the guard follows and refuses per hop",
        ),
        (
            "run-body-over-cap",
            "a body past the read ceiling, truncated and scored",
        ),
        (
            "http-transport-timeout-error",
            "a probe whose deadline elapsed",
        ),
        ("mcp-open-stream", "a stream the server never closes"),
        (
            "run-edge-530",
            "an edge status that speaks for the auditor, not the target",
        ),
        ("run-unreachable", "a target that answers nothing"),
    ];
    for (name, what) in semantics {
        let case = by_name
            .get(name)
            .unwrap_or_else(|| panic!("the corpus lost the {what} scenario ({name})"));
        match &case.golden {
            Golden::Unreachable(expected) => {
                let RunOutcome::Unreachable { reason, .. } =
                    replay(&case.scenario, Arc::new(default_handlers()))
                else {
                    panic!("{name}: scored where the site reported unreachable");
                };
                assert_eq!(&reason, expected, "{name}: {what}");
            }
            Golden::Scorecard(_) => {
                let actual = scorecard_json(case).unwrap_or_else(|e| panic!("{name}: {e}"));
                let expected = golden_text(name);
                assert!(
                    actual == expected,
                    "{name} ({what}) differs at {}",
                    first_difference(&expected, &actual)
                );
            }
        }
    }
}

/// A golden is only evidence if breaking it fails the suite. Mutating one
/// digit of a score in a copy of a golden must be caught by the same
/// comparison the suite runs.
#[test]
fn a_mutated_golden_fails_the_comparison() {
    let cases = load_cases();
    let case = cases
        .iter()
        .find(|c| c.name == "run-healthy")
        .expect("the healthy whole-run scenario");
    let expected = golden_text(&case.name);
    let actual = scorecard_json(case).expect("the healthy scenario scores");
    assert_eq!(actual, expected);

    let mutated = expected.replacen("\"score_pct\": 93", "\"score_pct\": 94", 1);
    assert_ne!(mutated, expected, "the mutation must change the document");
    assert_ne!(
        actual, mutated,
        "the comparison must reject a scorecard whose score moved"
    );
    let diff = first_difference(&mutated, &actual);
    assert!(
        diff.contains("score_pct"),
        "the failure must name the field: {diff}"
    );
}
