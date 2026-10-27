//! A row that evaluates several targets (the API anchors and the
//! descriptions they declare), a mirror of the target helpers in the site's
//! `handlers/shared.ts`: each target's own outcome and the row's verdict
//! over them.

use serde_json::json;

use crate::web_audit::engine::ProbeOutcome;
use crate::web_audit::engine::ProbeStatus;
use crate::web_audit::scorecard::{EvidenceItem, NaReason};

/// One target of a row that evaluates several, with its own outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetOutcome {
    /// The target's verdict.
    pub status: ProbeStatus,
    /// Why it is `na`, when it is.
    pub na_reason: Option<NaReason>,
    /// Its evidence.
    pub evidence: Vec<EvidenceItem>,
}

// Worst first: something there and wrong outranks a spec defect, which
// outranks nothing there, which outranks an operational unknown; a row
// passes only when every target it evaluated passed.
const TARGET_STATUS_RANK: [ProbeStatus; 5] = [
    ProbeStatus::Broken,
    ProbeStatus::Noncompliant,
    ProbeStatus::Absent,
    ProbeStatus::Error,
    ProbeStatus::Pass,
];

/// The worst status among `statuses`, ignoring targets not evaluated; `Na`
/// only when none was.
pub fn worst_target_status(statuses: &[ProbeStatus]) -> ProbeStatus {
    TARGET_STATUS_RANK
        .into_iter()
        .find(|s| statuses.contains(s))
        .unwrap_or(ProbeStatus::Na)
}

/// A row's outcome over its targets. Each evidence item carries its
/// target's `target_status` (and `na_reason`), where the row's per-host
/// outcomes are read from. A target not evaluated neither passes nor fails
/// the row; when none was, the row takes the first target's reason.
pub fn aggregate_targets(targets: Vec<TargetOutcome>) -> ProbeOutcome {
    let status = worst_target_status(&targets.iter().map(|t| t.status).collect::<Vec<_>>());
    let first_reason = targets.first().and_then(|t| t.na_reason);
    let mut evidence = Vec::new();
    for target in targets {
        for mut item in target.evidence {
            item.insert("target_status".into(), json!(target.status.as_str()));
            if let Some(reason) = target.na_reason {
                item.insert("na_reason".into(), json!(reason));
            }
            evidence.push(item);
        }
    }
    let mut outcome = ProbeOutcome::new(status, evidence);
    if status == ProbeStatus::Na {
        outcome.na_reason = first_reason;
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(status: ProbeStatus, reason: Option<NaReason>) -> TargetOutcome {
        TargetOutcome {
            status,
            na_reason: reason,
            evidence: vec![EvidenceItem::new()],
        }
    }

    #[test]
    fn the_row_takes_the_worst_evaluated_target_and_an_unevaluated_one_neither_passes_nor_fails() {
        let row = aggregate_targets(vec![
            target(ProbeStatus::Na, Some(NaReason::DeclaredHostBlocked)),
            target(ProbeStatus::Pass, None),
            target(ProbeStatus::Error, None),
        ]);
        assert_eq!(row.status, ProbeStatus::Error);
        assert_eq!(row.na_reason, None);
        let marks: Vec<_> = row
            .evidence
            .iter()
            .map(|i| (i["target_status"].clone(), i.get("na_reason").cloned()))
            .collect();
        assert_eq!(
            marks,
            [
                (json!("na"), Some(json!("declared-host-blocked"))),
                (json!("pass"), None),
                (json!("error"), None),
            ]
        );
        assert_eq!(
            worst_target_status(&[ProbeStatus::Absent, ProbeStatus::Noncompliant]),
            ProbeStatus::Noncompliant
        );
    }

    #[test]
    fn a_row_with_no_evaluated_target_is_na_with_the_first_reason() {
        let row = aggregate_targets(vec![
            target(ProbeStatus::Na, Some(NaReason::DeclaredHostUnreachable)),
            target(ProbeStatus::Na, Some(NaReason::DeclaredHostBlocked)),
        ]);
        assert_eq!(row.status, ProbeStatus::Na);
        assert_eq!(row.na_reason, Some(NaReason::DeclaredHostUnreachable));
    }
}
