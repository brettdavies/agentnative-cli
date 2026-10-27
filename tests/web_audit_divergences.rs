//! The divergence ledger's input: with `WEB_AUDIT_DIVERGENCE_OUT=<file>`,
//! writes every corpus element the engine does not reproduce as a JSON array
//! of `[scenario, element]` pairs. `scripts/web-audit-divergence-ledger.py`
//! runs this and attributes each element to the site change that set its
//! golden value. Without the variable it only checks that the dump is
//! deterministic.

#![cfg(feature = "web")]

mod common;

use common::corpus::load_cases;
use common::ledger::corpus_divergences;

#[test]
fn dump_corpus_divergences() {
    let cases = load_cases();
    let first = corpus_divergences(&cases);
    let Ok(path) = std::env::var("WEB_AUDIT_DIVERGENCE_OUT") else {
        assert_eq!(first, corpus_divergences(&cases), "replay is deterministic");
        return;
    };
    let pairs: Vec<[&str; 2]> = first
        .iter()
        .map(|(scenario, element)| [scenario.as_str(), element.as_str()])
        .collect();
    std::fs::write(&path, serde_json::to_string_pretty(&pairs).unwrap())
        .unwrap_or_else(|e| panic!("write {path}: {e}"));
}
