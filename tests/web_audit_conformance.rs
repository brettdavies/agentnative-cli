//! The parity claim, as CI evidence: every vendored corpus scenario
//! replays through the Rust engine and its scorecard must serialize byte
//! for byte to the JSON the anc.dev engine wrote for the same exchanges.
//!
//! The corpus is the site's own recording at the single-hop fetch seam
//! (`tests/fixtures/web-audit-conformance/README.md`), so a divergence
//! here is a divergence in a verdict, an evidence line, a score, or a key
//! order, not in a test double. An element the Rust engine cannot yet
//! reproduce at the vendored pin is an entry in the divergence ledger
//! (`tests/fixtures/web-audit-divergences.toml`, see `common::ledger`); the
//! observed set must equal the ledger, so nothing diverges silently and no
//! entry outlives its fix.

#![cfg(feature = "web")]

mod common;

use std::collections::BTreeMap;
use std::fs;

use agentnative::web_audit::engine::RunOutcome;
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::scorecard::to_wire_json;
use common::corpus::{Case, Golden, load_cases, replay};
use common::ledger::{compare_with_ledger, corpus_divergences, load_ledger};
use std::sync::Arc;

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
fn every_corpus_scenario_reproduces_the_site_scorecard_or_is_ledgered() {
    let cases = load_cases();
    assert_eq!(cases.len(), 138, "corpus scenarios (bump deliberately)");
    let unreachable = cases
        .iter()
        .filter(|c| matches!(c.golden, Golden::Unreachable(_)))
        .count();
    assert_eq!(
        unreachable, 3,
        "the corpus carries three unreachable goldens"
    );

    let actual = corpus_divergences(&cases);
    let ledger = load_ledger();
    let problems = compare_with_ledger(&actual, &ledger, |_| true);
    let mut detail: Vec<String> = Vec::new();
    for case in &cases {
        let unledgered = actual
            .iter()
            .any(|(s, e)| s == &case.name && !ledger.contains_key(&(s.clone(), e.clone())));
        if unledgered && let Ok(json) = scorecard_json(case) {
            detail.push(format!(
                "{} differs at {}",
                case.name,
                first_difference(&golden_text(&case.name), &json)
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "{} divergence ledger mismatch(es):\n{}\n{}",
        problems.len(),
        problems.join("\n"),
        detail.join("\n")
    );
    let matching = cases
        .iter()
        .filter(|c| !actual.iter().any(|(s, _)| s == &c.name))
        .count();
    assert!(matching > 0, "no scenario reproduces byte for byte");
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
    let ledger = load_ledger();
    for (name, what) in semantics {
        let case = by_name
            .get(name)
            .unwrap_or_else(|| panic!("the corpus lost the {what} scenario ({name})"));
        if ledger.keys().any(|(scenario, _)| scenario == name) {
            continue;
        }
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
/// A golden is only evidence if breaking it fails the suite: mutating an
/// element the engine reproduces must surface as a divergence in that
/// element, which the ledger does not hold.
#[test]
fn a_mutated_golden_fails_the_comparison() {
    let ledger = load_ledger();
    let cases = load_cases();
    let case = cases
        .iter()
        .find(|c| c.name == "run-healthy")
        .expect("the healthy whole-run scenario");
    let element = ("run-healthy".to_string(), "spec_version".to_string());
    assert!(
        !ledger.contains_key(&element),
        "the anchor element must be one the engine reproduces"
    );
    let expected = golden_text(&case.name);
    let actual = scorecard_json(case).expect("the healthy scenario scores");
    let spec = format!("\"spec_version\": \"{}\"", case.scenario.spec_version);
    let mutated = expected.replacen(&spec, "\"spec_version\": \"0.0.0\"", 1);
    assert_ne!(mutated, expected, "the mutation must change the document");
    assert_ne!(
        actual, mutated,
        "the comparison must reject a scorecard whose field moved"
    );
    let diff = first_difference(&mutated, &actual);
    assert!(
        diff.contains("spec_version"),
        "the failure must name the field: {diff}"
    );
}
