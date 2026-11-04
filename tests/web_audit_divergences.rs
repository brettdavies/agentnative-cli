//! The divergence ledger's input: with `WEB_AUDIT_DIVERGENCE_OUT=<file>`,
//! writes every corpus element the engine does not reproduce as a JSON array
//! of `[scenario, element]` pairs. `scripts/web-audit-divergence-ledger.py`
//! runs this and attributes each element to the site change that set its
//! golden value. Without the variable it only checks that the dump is
//! deterministic. With `WEB_AUDIT_UNPORTED_OUT=<file>`, also writes each handler
//! kind and eval rule this build cannot run, with the checks that bind it, for a
//! bump PR's body.

#![cfg(feature = "web")]

mod common;

use std::collections::BTreeMap;

use agentnative::web_audit::registry::{CHECKS, EvalBinding, HandlerBinding};
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

#[test]
fn dump_unported_bindings() {
    let Ok(path) = std::env::var("WEB_AUDIT_UNPORTED_OUT") else {
        return;
    };
    let mut handlers: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut evals: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for check in CHECKS {
        if let HandlerBinding::Unsupported(kind) = check.handler {
            handlers.entry(kind).or_default().push(check.id);
        }
        if let Some(EvalBinding::Unsupported(rule)) = check.eval {
            evals.entry(rule).or_default().push(check.id);
        }
    }
    let doc = serde_json::json!({"handlers": handlers, "evals": evals});
    std::fs::write(&path, serde_json::to_string_pretty(&doc).unwrap())
        .unwrap_or_else(|e| panic!("write {path}: {e}"));
}
