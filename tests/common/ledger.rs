//! The divergence ledger (`tests/fixtures/web-audit-divergences.toml`): the
//! exact set of corpus elements the engine does not yet reproduce at the
//! vendored site pin, each owned by the site change a later port closes.
//! The suites compare the replayed corpus with it in both directions, so an
//! unledgered divergence fails and a ledgered element that now matches fails
//! until its entry is removed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use agentnative::web_audit::engine::RunOutcome;
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::registry::SITE_SHA;
use agentnative::web_audit::scorecard::to_wire_json;
use serde::Deserialize;
use serde_json::Value;

use super::corpus::{Case, Golden, corpus_dir, replay};

/// The element key an unreachable-outcome mismatch is recorded under.
pub const UNREACHABLE: &str = "unreachable";
/// The element key for a document whose fields all match but whose bytes do
/// not (key order, whitespace).
pub const DOCUMENT: &str = "document";
/// The prefix a result row's element key carries before its check id.
pub const ROW: &str = "row:";

/// `(scenario, element)`: a top-level scorecard field, `row:<check id>`,
/// `unreachable`, or `document`.
pub type Element = (String, String);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LedgerFile {
    site_sha: String,
    #[serde(default)]
    divergence: Vec<Entry>,
}

/// One ledger entry.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub scenario: String,
    pub element: String,
    /// The site change that owns the divergence, as `#<PR>`.
    pub owner: String,
}

pub fn ledger_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/web-audit-divergences.toml")
}

/// The committed ledger, keyed by element. Fails when it was generated at a
/// different pin, or is unsorted or duplicated.
pub fn load_ledger() -> BTreeMap<Element, String> {
    let raw = fs::read_to_string(ledger_path()).expect("the divergence ledger");
    let file: LedgerFile = toml::from_str(&raw).expect("the divergence ledger parses");
    assert_eq!(
        file.site_sha, SITE_SHA,
        "the divergence ledger was generated at another site pin; regenerate it"
    );
    let keys: Vec<Element> = file
        .divergence
        .iter()
        .map(|e| (e.scenario.clone(), e.element.clone()))
        .collect();
    assert!(
        keys.windows(2).all(|w| w[0] < w[1]),
        "the divergence ledger must be sorted by (scenario, element) without duplicates"
    );
    file.divergence
        .into_iter()
        .map(|e| {
            assert!(
                e.owner.starts_with('#') && e.owner[1..].parse::<u32>().is_ok(),
                "{}/{}: owner {:?} must name a site PR as #<number>",
                e.scenario,
                e.element,
                e.owner
            );
            ((e.scenario, e.element), e.owner)
        })
        .collect()
}

fn golden_text(name: &str) -> String {
    fs::read_to_string(
        corpus_dir()
            .join("scenarios")
            .join(name)
            .join("scorecard.json"),
    )
    .expect("a golden scorecard")
}

/// The elements of one scenario the engine does not reproduce.
pub fn divergences_of(case: &Case) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let outcome = replay(&case.scenario, Arc::new(default_handlers()));
    match (&case.golden, outcome) {
        (Golden::Unreachable(want), RunOutcome::Unreachable { reason, .. }) => {
            if want != &reason {
                out.insert(UNREACHABLE.to_string());
            }
        }
        (Golden::Unreachable(_), RunOutcome::Complete(_))
        | (Golden::Scorecard(_), RunOutcome::Unreachable { .. }) => {
            out.insert(UNREACHABLE.to_string());
        }
        (Golden::Scorecard(_), RunOutcome::Complete(report)) => {
            let want_text = golden_text(&case.name);
            let got_text = to_wire_json(&report.scorecard);
            if want_text == got_text {
                return out;
            }
            let want: Value = serde_json::from_str(&want_text).expect("golden JSON");
            let got: Value = serde_json::from_str(&got_text).expect("engine JSON");
            for (key, value) in want.as_object().expect("a scorecard object") {
                if key != "results" && got.get(key) != Some(value) {
                    out.insert(key.clone());
                }
            }
            let rows = |doc: &Value| -> BTreeMap<String, Value> {
                doc["results"]
                    .as_array()
                    .map(|rows| {
                        rows.iter()
                            .map(|r| (r["id"].as_str().unwrap_or_default().to_string(), r.clone()))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let (want_rows, got_rows) = (rows(&want), rows(&got));
            for id in want_rows.keys().chain(got_rows.keys()) {
                if want_rows.get(id) != got_rows.get(id) {
                    out.insert(format!("{ROW}{id}"));
                }
            }
            if out.is_empty() {
                out.insert(DOCUMENT.to_string());
            }
        }
    }
    out
}

/// Every divergent element across the corpus.
pub fn corpus_divergences(cases: &[Case]) -> BTreeSet<Element> {
    cases
        .iter()
        .flat_map(|case| {
            divergences_of(case)
                .into_iter()
                .map(|element| (case.name.clone(), element))
        })
        .collect()
}

/// Compare an observed divergence set with the ledger entries in scope.
/// Returns one line per unledgered divergence and per stale entry.
pub fn compare_with_ledger(
    actual: &BTreeSet<Element>,
    ledger: &BTreeMap<Element, String>,
    in_scope: impl Fn(&Element) -> bool,
) -> Vec<String> {
    let mut problems = Vec::new();
    for element in actual {
        if !ledger.contains_key(element) {
            problems.push(format!(
                "{}/{}: diverges from the site and has no ledger entry",
                element.0, element.1
            ));
        }
    }
    for (element, owner) in ledger {
        if in_scope(element) && !actual.contains(element) {
            problems.push(format!(
                "{}/{}: now matches the site; remove this entry (owner {owner}) from tests/fixtures/web-audit-divergences.toml",
                element.0, element.1
            ));
        }
    }
    problems
}
