//! The hosts each row's evidence came from, a mirror of `rowHostFields` in
//! the site's `provenance.ts`.

use std::collections::HashMap;

use serde_json::Value;

use super::api_trail::host_of;
use super::types::ProbeStatus;
use crate::web_audit::handlers::targets::worst_target_status;
use crate::web_audit::scorecard::{EvidenceItem, NaReason, RowHost, ScorecardStatus};

/// The host an item's URL names, else the host it records outright.
fn item_host(item: &EvidenceItem) -> Option<String> {
    match item.get("url").and_then(Value::as_str) {
        Some(url) => host_of(url),
        None => item
            .get("host")
            .and_then(Value::as_str)
            .filter(|h| !h.is_empty())
            .map(str::to_string),
    }
}

fn host_entry(host: String, outcome: Option<&(Vec<ProbeStatus>, Option<NaReason>)>) -> RowHost {
    let Some((statuses, na_reason)) = outcome else {
        return RowHost {
            host,
            status: None,
            na_reason: None,
        };
    };
    let status = worst_target_status(statuses);
    RowHost {
        host,
        status: Some(status.to_scorecard()),
        na_reason: if status.to_scorecard() == ScorecardStatus::NA {
            *na_reason
        } else {
            None
        },
    }
}

/// The distinct hosts a row's evidence was requested from, in evidence
/// order, plus `host` when there is exactly one. An item the guard refused
/// never reached its host, so it names none. A row that evaluated several
/// targets on more than one host (the API anchors) gives each host its own
/// outcome, read from its items' `target_status`.
pub fn row_host_fields(evidence: &[EvidenceItem]) -> (Vec<RowHost>, Option<String>) {
    let mut hosts: Vec<String> = Vec::new();
    let mut outcomes: HashMap<String, (Vec<ProbeStatus>, Option<NaReason>)> = HashMap::new();
    for item in evidence {
        if item.contains_key("blocked") {
            continue;
        }
        let Some(host) = item_host(item) else {
            continue;
        };
        if !hosts.contains(&host) {
            hosts.push(host.clone());
        }
        let target = item.get("target_status").and_then(Value::as_str);
        let Some(status) = ProbeStatus::ALL
            .into_iter()
            .find(|s| Some(s.as_str()) == target)
        else {
            continue;
        };
        let outcome = outcomes.entry(host).or_default();
        outcome.0.push(status);
        if outcome.1.is_none() {
            outcome.1 = item
                .get("na_reason")
                .and_then(|r| serde_json::from_value(r.clone()).ok());
        }
    }
    let per_host = hosts.len() > 1 && hosts.iter().all(|h| outcomes.contains_key(h));
    let single = (hosts.len() == 1).then(|| hosts[0].clone());
    let rows = hosts
        .into_iter()
        .map(|host| {
            let outcome = if per_host { outcomes.get(&host) } else { None };
            host_entry(host, outcome)
        })
        .collect();
    (rows, single)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn items(values: Value) -> Vec<EvidenceItem> {
        values
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_object().cloned().unwrap())
            .collect()
    }

    #[test]
    fn a_row_over_several_hosts_gives_each_its_own_outcome() {
        let evidence = items(json!([
            {"url": "https://a.test/x", "target_status": "pass"},
            {"url": "https://a.test/y", "target_status": "absent"},
            {"host": "b.test", "target_status": "na", "na_reason": "declared-host-unreachable"},
            {"url": "https://c.test/", "blocked": "not https", "target_status": "absent"},
        ]));
        let (hosts, single) = row_host_fields(&evidence);
        assert_eq!(single, None);
        let wire = serde_json::to_value(&hosts).unwrap();
        assert_eq!(
            wire,
            json!([
                {"host": "a.test", "status": "absent"},
                {"host": "b.test", "status": "n_a", "na_reason": "declared-host-unreachable"},
            ])
        );
    }

    #[test]
    fn one_host_or_a_host_without_a_target_outcome_reads_bare() {
        let (hosts, single) = row_host_fields(&items(
            json!([{"url": "https://a.test:8443/x", "target_status": "pass"}]),
        ));
        assert_eq!(single.as_deref(), Some("a.test:8443"));
        assert_eq!(hosts[0].status, None);
        let (hosts, _) = row_host_fields(&items(json!([
            {"url": "https://a.test/x", "target_status": "pass"},
            {"url": "https://b.test/x"},
        ])));
        assert!(hosts.iter().all(|h| h.status.is_none()));
    }
}
