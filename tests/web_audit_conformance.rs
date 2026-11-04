//! The parity claim, as CI evidence: every vendored corpus scenario
//! replays through the Rust engine and its scorecard must serialize byte
//! for byte to the JSON the anc.dev engine wrote for the same exchanges.
//!
//! The corpus is the site's own recording at the single-hop fetch seam
//! (`tests/fixtures/web-audit-contract/conformance/README.md`), so a divergence
//! here is a divergence in a verdict, an evidence line, a score, or a key
//! order, not in a test double. An element the Rust engine cannot yet
//! reproduce at the vendored pin is an entry in the divergence ledger
//! (`tests/fixtures/web-audit-divergences.toml`, see `common::ledger`); the
//! observed set must equal the ledger, so nothing diverges silently and no
//! entry outlives its fix.

#![cfg(feature = "web")]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use agentnative::web_audit::engine::RunOutcome;
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::registry::CHECKS;
use agentnative::web_audit::scorecard::{ScorecardStatus, to_wire_json};
use common::corpus::{Case, Golden, SentRequest, corpus_dir, load_cases, replay};
use common::ledger::{
    compare_with_ledger, corpus_divergences, divergences_against, divergences_of, element,
    golden_text, load_ledger, request_differences, requests_differ,
};
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

#[test]
fn every_corpus_scenario_reproduces_the_site_scorecard_or_is_ledgered() {
    let cases = load_cases();
    assert!(
        cases
            .iter()
            .any(|c| matches!(c.golden, Golden::Unreachable(_))),
        "the corpus carries unreachable goldens"
    );

    let actual = corpus_divergences(&cases);
    let ledger = load_ledger();
    let problems = compare_with_ledger(&actual, &ledger, |_| true);
    let mut detail: Vec<String> = Vec::new();
    for case in &cases {
        let unledgered: Vec<&String> = actual
            .iter()
            .filter(|(s, e)| s == &case.name && !ledger.contains_key(&(s.clone(), e.clone())))
            .map(|(_, e)| e)
            .collect();
        if unledgered.iter().any(|e| *e == common::ledger::REQUESTS) {
            let transport = common::corpus::transport_for(&case.scenario);
            let _ = common::corpus::replay_on(
                &case.scenario,
                Arc::new(default_handlers()),
                transport.clone(),
            );
            let sent: Vec<SentRequest> =
                transport.requests().iter().map(SentRequest::from).collect();
            detail.push(format!(
                "{} requests:\n{}",
                case.name,
                request_differences(&sent, &common::corpus::recorded_requests(&case.name))
                    .join("\n")
            ));
        }
        if unledgered.iter().any(|e| *e != common::ledger::REQUESTS)
            && let Ok(json) = scorecard_json(case)
        {
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

/// A check no scenario is the subject of has no parity evidence behind it.
#[test]
fn every_check_is_the_subject_of_some_scenario() {
    let covered: BTreeSet<String> = load_cases()
        .into_iter()
        .flat_map(|case| case.scenario.covers)
        .collect();
    for check in CHECKS {
        assert!(
            covered.contains(check.id),
            "{}: no corpus scenario covers it",
            check.id
        );
    }
    for id in &covered {
        assert!(
            CHECKS.iter().any(|check| check.id == id),
            "a scenario covers {id}, which the registry does not hold"
        );
    }
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

/// A golden is only evidence if breaking it fails the suite: mutating an
/// element must surface as a divergence in exactly that element, through the
/// comparison the suite runs.
#[test]
fn a_mutated_golden_fails_the_comparison() {
    let cases = load_cases();
    let case = cases
        .iter()
        .find(|c| c.name == "run-healthy")
        .expect("the healthy whole-run scenario");
    let golden = golden_text(&case.name);
    let spec = format!("\"spec_version\": \"{}\"", case.scenario.spec_version);
    let mutated = golden.replacen(&spec, "\"spec_version\": \"0.0.0\"", 1);
    assert_ne!(mutated, golden, "the mutation must change the document");
    // `document` only reads when no field differs, and this mutation makes one differ.
    let mut expected = divergences_of(case);
    expected.remove("document");
    assert!(
        expected.insert("spec_version".to_string()),
        "the anchor element must be one the engine reproduces"
    );
    assert_eq!(divergences_against(case, &mutated), expected);
}

#[test]
fn a_field_only_the_engine_writes_is_a_divergence() {
    let cases = load_cases();
    let case = cases
        .iter()
        .find(|c| c.name == "run-healthy")
        .expect("the healthy whole-run scenario");
    let mut golden: serde_json::Value =
        serde_json::from_str(&golden_text(&case.name)).expect("golden JSON");
    golden
        .as_object_mut()
        .expect("a scorecard object")
        .remove("spec_version");
    let without = serde_json::to_string_pretty(&golden).unwrap();
    assert!(divergences_against(case, &without).contains("spec_version"));
}

fn sent(url: &str) -> SentRequest {
    SentRequest {
        method: "GET".to_string(),
        url: url.to_string(),
        headers: BTreeMap::from([("accept".to_string(), "*/*".to_string())]),
        body: None,
    }
}

#[test]
fn a_request_set_compares_as_parsed_entries_and_each_request_once() {
    let recorded = BTreeSet::from([sent("https://a/"), sent("https://b/")]);
    let same = [sent("https://b/"), sent("https://a/")];
    assert!(!requests_differ(&same, &recorded), "order does not count");
    assert!(
        requests_differ(&[sent("https://a/")], &recorded),
        "a request the port never sent"
    );
    let extra = [sent("https://a/"), sent("https://b/"), sent("https://c/")];
    assert!(
        requests_differ(&extra, &recorded),
        "a request the site never sent"
    );
    let twice = [sent("https://a/"), sent("https://a/"), sent("https://b/")];
    assert!(requests_differ(&twice, &recorded), "a request sent twice");
    let mut other_header = sent("https://b/");
    other_header
        .headers
        .insert("accept".to_string(), "text/html".to_string());
    assert!(
        requests_differ(&[sent("https://a/"), other_header], &recorded),
        "a header value"
    );
    let mut other_body = sent("https://b/");
    other_body.body = Some("{}".to_string());
    assert!(
        requests_differ(&[sent("https://a/"), other_body], &recorded),
        "a body"
    );
}

/// A site that records a request a replay does not send shows up under the scenario's
/// `requests` element, which the ledger holds like any other.
#[test]
fn a_request_the_replay_does_not_reproduce_reads_as_the_requests_element() {
    let cases = load_cases();
    let case = cases
        .iter()
        .find(|c| c.name == "run-healthy")
        .expect("the healthy whole-run scenario");
    assert!(!common::corpus::recorded_requests(&case.name).is_empty());
    let transport = common::corpus::transport_for(&case.scenario);
    let _ = common::corpus::replay_on(
        &case.scenario,
        Arc::new(default_handlers()),
        transport.clone(),
    );
    let replayed: Vec<SentRequest> = transport.requests().iter().map(SentRequest::from).collect();
    // Compared with the replay itself: a bump may ledger this scenario's requests.
    let mut recorded: BTreeSet<SentRequest> = replayed.iter().cloned().collect();
    assert!(!requests_differ(&replayed, &recorded));
    let dropped = recorded.iter().next().cloned().unwrap();
    recorded.remove(&dropped);
    assert!(requests_differ(&replayed, &recorded));

    let actual = BTreeSet::from([element(&case.name, common::ledger::REQUESTS)]);
    assert_eq!(
        compare_with_ledger(&actual, &BTreeMap::new(), |_| true).len(),
        1
    );
    let ledger = BTreeMap::from([(
        element(&case.name, common::ledger::REQUESTS),
        "#1".to_string(),
    )]);
    assert!(compare_with_ledger(&actual, &ledger, |_| true).is_empty());
    assert_eq!(
        compare_with_ledger(&BTreeSet::new(), &ledger, |_| true).len(),
        1
    );
}

/// A dropped directory or a filter bug in the loader would shrink every suite's
/// input without failing any of them; the corpus's own index names each scenario.
#[test]
fn the_loader_reads_every_scenario_the_corpus_indexes() {
    let raw = fs::read_to_string(corpus_dir().join("scores.json")).expect("scores.json");
    let index: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(&raw).expect("scores.json parses");
    let indexed: BTreeSet<String> = index.into_keys().collect();
    let loaded: BTreeSet<String> = load_cases().into_iter().map(|case| case.name).collect();
    assert_eq!(loaded, indexed);
}

/// A check this build cannot run skips where the site scores it, so the bump
/// that brought it lands ledgered. One the corpus settles n_a everywhere never reaches
/// its handler, so nothing diverges; `web-audit-unported` still holds the release.
#[test]
fn every_unported_binding_the_site_scores_is_ledgered() {
    let ledger = load_ledger();
    let cases = load_cases();
    for check in CHECKS.iter().filter(|check| !check.is_ported()) {
        let scored = cases.iter().any(|case| match &case.golden {
            Golden::Scorecard(card) => card
                .results
                .iter()
                .any(|row| row.id == check.id && row.status != ScorecardStatus::NA),
            Golden::Unreachable(_) => false,
        });
        let element = format!("{}{}", common::ledger::ROW, check.id);
        assert!(
            !scored || ledger.keys().any(|(_, e)| *e == element),
            "{}: scored by the site, bound Unsupported, and no {element} ledger entry",
            check.id
        );
    }
}

#[test]
fn a_requests_divergence_names_what_differs() {
    let request = |url: &str| SentRequest {
        method: "GET".to_string(),
        url: url.to_string(),
        headers: BTreeMap::new(),
        body: None,
    };
    let sent = [
        request("https://a.test/sent"),
        request("https://a.test/twice"),
        request("https://a.test/twice"),
    ];
    let recorded = BTreeSet::from([
        request("https://a.test/recorded"),
        request("https://a.test/twice"),
    ]);
    let lines = request_differences(&sent, &recorded);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("sent, not recorded") && l.contains("/sent")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("recorded, not sent") && l.contains("/recorded")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("sent twice") && l.contains("/twice")),
        "{lines:?}"
    );
}
