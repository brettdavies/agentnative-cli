//! The divergence ledger's own rules: how it parses, how it compares with what a run
//! found, and which differences in a golden's rows the comparison has to see.

#![cfg(feature = "web")]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::corpus::{Case, load_cases};
use common::ledger::{
    ROW, compare_with_ledger, divergences_against, element, golden_text, parse_ledger,
};
use serde_json::{Value, json};

#[test]
fn an_unledgered_divergence_and_a_stale_entry_are_both_problems() {
    let actual = BTreeSet::from([element("a", "score")]);
    let ledger = BTreeMap::from([(element("b", "score"), "#1".to_string())]);
    let problems = compare_with_ledger(&actual, &ledger, |_| true);
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(problems[0].starts_with("a/score: diverges"), "{problems:?}");
    assert!(
        problems[1].starts_with("b/score: now matches"),
        "{problems:?}"
    );
}

#[test]
fn a_stale_entry_outside_the_suites_scope_is_not_a_problem() {
    let ledger = BTreeMap::from([(element("b", "score"), "#1".to_string())]);
    let problems = compare_with_ledger(&BTreeSet::new(), &ledger, |_| false);
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn a_ledgered_divergence_is_not_a_problem() {
    let actual = BTreeSet::from([element("a", "score")]);
    let ledger = BTreeMap::from([(element("a", "score"), "#1".to_string())]);
    assert!(compare_with_ledger(&actual, &ledger, |_| true).is_empty());
}

fn healthy_case() -> Case {
    load_cases()
        .into_iter()
        .find(|c| c.name == "run-healthy")
        .expect("the healthy whole-run scenario")
}

fn healthy_golden() -> Value {
    serde_json::from_str(&golden_text("run-healthy")).expect("golden JSON")
}

/// The golden's first row's id, and the golden with `edit` applied to its rows.
fn golden_with_rows(edit: impl FnOnce(&mut Vec<Value>)) -> (String, String) {
    let mut golden = healthy_golden();
    let rows = golden["results"].as_array_mut().expect("result rows");
    let id = rows[0]["id"].as_str().expect("a row id").to_string();
    edit(rows);
    (id, serde_json::to_string_pretty(&golden).unwrap())
}

#[test]
fn a_row_only_the_engine_writes_is_a_divergence() {
    let (id, golden) = golden_with_rows(|rows| {
        rows.remove(0);
    });
    assert!(divergences_against(&healthy_case(), &golden).contains(&format!("{ROW}{id}")));
}

#[test]
fn a_row_only_the_golden_holds_is_a_divergence() {
    let (_, golden) =
        golden_with_rows(|rows| rows.push(json!({ "id": "no-such-check", "status": "pass" })));
    assert!(divergences_against(&healthy_case(), &golden).contains(&format!("{ROW}no-such-check")));
}

#[test]
fn a_row_whose_status_moved_is_a_divergence() {
    let (id, golden) = golden_with_rows(|rows| rows[0]["status"] = json!("moved"));
    assert!(divergences_against(&healthy_case(), &golden).contains(&format!("{ROW}{id}")));
}

#[test]
fn a_field_only_the_golden_holds_is_a_divergence() {
    let mut golden = healthy_golden();
    golden["no_such_field"] = json!(1);
    let text = serde_json::to_string_pretty(&golden).unwrap();
    assert!(divergences_against(&healthy_case(), &text).contains("no_such_field"));
}

const PIN: &str = "0123456789abcdef0123456789abcdef01234567";
const OTHER_PIN: &str = "76543210fedcba9876543210fedcba9876543210";
const ENTRY: &str =
    "[[divergence]]\nscenario = \"run-healthy\"\nelement = \"score\"\nowner = \"#1\"\n";

#[test]
fn an_empty_ledger_holds_at_any_pin() {
    let ledger = parse_ledger("# header only\n", PIN).expect("an empty ledger names no pin");
    assert!(ledger.is_empty());
}

#[test]
fn a_ledger_with_entries_holds_only_at_its_own_pin() {
    let at = |pin: &str| format!("site_sha = \"{pin}\"\n{ENTRY}");
    assert_eq!(parse_ledger(&at(PIN), PIN).expect("its own pin").len(), 1);
    let stale = parse_ledger(&at(OTHER_PIN), PIN).expect_err("another pin");
    assert!(
        stale.contains(OTHER_PIN) && stale.contains("regenerate"),
        "{stale}"
    );
    let unnamed = parse_ledger(ENTRY, PIN).expect_err("entries with no pin");
    assert!(unnamed.contains("regenerate"), "{unnamed}");
}

#[test]
fn an_empty_ledger_naming_a_pin_is_refused() {
    let raw = format!("site_sha = \"{PIN}\"\n");
    let problem = parse_ledger(&raw, PIN).expect_err("a stale claim waiting to happen");
    assert!(problem.contains("names no site pin"), "{problem}");
}

#[test]
fn an_unsorted_or_duplicated_ledger_is_refused() {
    let row = |scenario: &str| {
        format!("[[divergence]]\nscenario = \"{scenario}\"\nelement = \"score\"\nowner = \"#1\"\n")
    };
    let pinned = format!("site_sha = \"{PIN}\"\n");
    for rows in [row("b") + &row("a"), row("a") + &row("a")] {
        let problem = parse_ledger(&(pinned.clone() + &rows), PIN).expect_err("out of order");
        assert!(problem.contains("sorted"), "{problem}");
    }
}

#[test]
fn an_owner_that_names_no_site_pr_is_refused() {
    let raw = format!(
        "site_sha = \"{PIN}\"\n[[divergence]]\nscenario = \"a\"\nelement = \"score\"\nowner = \"someone\"\n"
    );
    let problem = parse_ledger(&raw, PIN).expect_err("no PR number");
    assert!(problem.contains("must name a site PR"), "{problem}");
}
