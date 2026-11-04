//! The divergence ledger (`tests/fixtures/web-audit-divergences.toml`): the
//! exact set of corpus elements the engine does not yet reproduce at the
//! vendored site pin, each owned by the site change a later port closes.
//! The suites compare the replayed corpus with it in both directions, so an
//! unledgered divergence fails and a ledgered element that now matches fails
//! until its entry is removed. Entries are true only at the pin that produced
//! them, so they name it; an empty ledger makes no claim about any pin and
//! names none, which lets a re-pin that reproduces the corpus touch no ledger.

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

use super::corpus::{
    Case, Golden, SentRequest, corpus_dir, recorded_requests, replay_on, transport_for,
};

/// The element key an unreachable-outcome mismatch is recorded under.
pub const UNREACHABLE: &str = "unreachable";
/// The element key for a document whose fields all match but whose bytes do
/// not (key order, whitespace).
pub const DOCUMENT: &str = "document";
/// The prefix a result row's element key carries before its check id.
pub const ROW: &str = "row:";

/// The element key for a scenario whose distinct requests differ from its
/// `requests.json`.
pub const REQUESTS: &str = "requests";

/// `(scenario, element)`: a top-level scorecard field, `row:<check id>`,
/// `unreachable`, `document`, or `requests`.
pub type Element = (String, String);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LedgerFile {
    site_sha: Option<String>,
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

/// The committed ledger, keyed by element. Fails when it does not hold at the
/// vendored pin, or is unsorted or duplicated.
pub fn load_ledger() -> BTreeMap<Element, String> {
    let raw = fs::read_to_string(ledger_path()).expect("the divergence ledger");
    parse_ledger(&raw, SITE_SHA).unwrap_or_else(|problem| panic!("{problem}"))
}

/// Reads a ledger against the vendored `pin`.
pub fn parse_ledger(raw: &str, pin: &str) -> Result<BTreeMap<Element, String>, String> {
    let file: LedgerFile =
        toml::from_str(raw).map_err(|e| format!("the divergence ledger does not parse: {e}"))?;
    match (file.divergence.is_empty(), file.site_sha.as_deref()) {
        (true, None) => {}
        (true, Some(named)) => {
            return Err(format!(
                "an empty divergence ledger names no site pin; remove site_sha = {named:?}"
            ));
        }
        (false, None) => {
            return Err(
                "the divergence ledger has entries and names no site pin; regenerate it"
                    .to_string(),
            );
        }
        (false, Some(named)) if named != pin => {
            return Err(format!(
                "the divergence ledger was generated at site pin {named}, not {pin}; regenerate it"
            ));
        }
        (false, Some(_)) => {}
    }
    let keys: Vec<Element> = file
        .divergence
        .iter()
        .map(|e| (e.scenario.clone(), e.element.clone()))
        .collect();
    if !keys.windows(2).all(|w| w[0] < w[1]) {
        return Err(
            "the divergence ledger must be sorted by (scenario, element) without duplicates"
                .to_string(),
        );
    }
    file.divergence
        .into_iter()
        .map(|e| {
            if e.owner.starts_with('#') && e.owner[1..].parse::<u32>().is_ok() {
                Ok(((e.scenario, e.element), e.owner))
            } else {
                Err(format!(
                    "{}/{}: owner {:?} must name a site PR as #<number>",
                    e.scenario, e.element, e.owner
                ))
            }
        })
        .collect()
}

pub fn golden_text(name: &str) -> String {
    fs::read_to_string(
        corpus_dir()
            .join("scenarios")
            .join(name)
            .join("scorecard.json"),
    )
    .expect("a golden scorecard")
}

/// A ledger key.
pub fn element(scenario: &str, element: &str) -> (String, String) {
    (scenario.to_string(), element.to_string())
}

/// Whether a run's requests differ from its recorded set. The set compares as parsed
/// entries, so header order and name case do not count. A request sent twice counts,
/// because the shared request memo should have answered the second send; a second send
/// the memo's own rules require, such as a body cut at a smaller cap than a later
/// reader's, is a ledger entry the port closes or the owner accepts.
pub fn requests_differ(sent: &[SentRequest], recorded: &BTreeSet<SentRequest>) -> bool {
    let distinct: BTreeSet<&SentRequest> = sent.iter().collect();
    distinct.len() != sent.len() || !distinct.iter().copied().eq(recorded.iter())
}

/// What a `requests` divergence consists of, one line per request.
pub fn request_differences(sent: &[SentRequest], recorded: &BTreeSet<SentRequest>) -> Vec<String> {
    let mut distinct = BTreeSet::new();
    let mut lines = Vec::new();
    for request in sent {
        if !distinct.insert(request) {
            lines.push(format!("sent twice: {request:?}"));
        }
    }
    lines.extend(
        distinct
            .iter()
            .filter(|request| !recorded.contains(**request))
            .map(|request| format!("sent, not recorded: {request:?}")),
    );
    lines.extend(
        recorded
            .iter()
            .filter(|request| !distinct.contains(request))
            .map(|request| format!("recorded, not sent: {request:?}")),
    );
    lines
}

/// The elements of one scenario the engine does not reproduce.
pub fn divergences_of(case: &Case) -> BTreeSet<String> {
    divergences_against(case, &golden_text(&case.name))
}

/// The same, against `want_text` in place of a scored scenario's golden document.
/// An unreachable scenario's golden is its reason, read from `case`.
pub fn divergences_against(case: &Case, want_text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let transport = transport_for(&case.scenario);
    let outcome = replay_on(
        &case.scenario,
        Arc::new(default_handlers()),
        transport.clone(),
    );
    let sent: Vec<SentRequest> = transport.requests().iter().map(SentRequest::from).collect();
    if requests_differ(&sent, &recorded_requests(&case.name)) {
        out.insert(REQUESTS.to_string());
    }
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
            let got_text = to_wire_json(&report.scorecard);
            if want_text == got_text {
                return out;
            }
            let want: Value = serde_json::from_str(want_text).expect("golden JSON");
            let got: Value = serde_json::from_str(&got_text).expect("engine JSON");
            for (key, value) in want.as_object().expect("a scorecard object") {
                if key != "results" && got.get(key) != Some(value) {
                    out.insert(key.clone());
                }
            }
            for key in got.as_object().expect("a scorecard object").keys() {
                if want.get(key).is_none() {
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
            if out.iter().all(|element| element == REQUESTS) {
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
                "{}/{}: now matches the site; remove this entry (owner {owner}) from tests/fixtures/web-audit-divergences.toml, and site_sha with the last one",
                element.0, element.1
            ));
        }
    }
    problems
}
