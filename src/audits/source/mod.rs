// Every audit under this module reads arbitrary UTF-8 from the audited tree.
// A byte-indexed slice is sound only when its bounds come from a scan of that
// same text, and each such site carries an `expect` naming the scan.
#![deny(clippy::string_slice)]

pub mod python;
pub mod rust;

use crate::audit::Audit;
use crate::project::Language;
use crate::types::{AuditLayer, AuditResult, AuditStatus};

/// Returns all source audits for the given language.
pub fn all_source_audits(language: Language) -> Vec<Box<dyn Audit>> {
    match language {
        Language::Rust => rust::all_rust_audits(),
        Language::Python => python::all_python_audits(),
        _ => vec![],
    }
}

/// Fold source results that two languages' audits produced under one id, so
/// each requirement keeps one row: the most severe status stands, carrying
/// the evidence of every result at that status. Rows keep their first
/// position.
pub fn merge_shared(results: Vec<AuditResult>) -> Vec<AuditResult> {
    let mut merged: Vec<AuditResult> = Vec::with_capacity(results.len());
    for result in results {
        let shared = merged.iter_mut().find(|seen| {
            seen.layer == AuditLayer::Source
                && result.layer == AuditLayer::Source
                && seen.id == result.id
        });
        match shared {
            Some(seen) => fold(seen, result),
            None => merged.push(result),
        }
    }
    merged
}

fn fold(seen: &mut AuditResult, other: AuditResult) {
    let (kept, incoming) = (severity(&seen.status), severity(&other.status));
    if incoming > kept {
        *seen = other;
    } else if incoming == kept
        && let (Some(into), Some(more)) = (evidence_mut(&mut seen.status), evidence(&other.status))
        && !more.is_empty()
        && into.as_str() != more
    {
        into.push('\n');
        into.push_str(more);
    }
}

fn severity(status: &AuditStatus) -> u8 {
    match status {
        AuditStatus::Error(_) => 6,
        AuditStatus::Fail(_) => 5,
        AuditStatus::Warn(_) => 4,
        AuditStatus::Pass => 3,
        AuditStatus::OptOut(_) => 2,
        AuditStatus::NotApplicable(_) => 1,
        AuditStatus::Skip(_) => 0,
    }
}

fn evidence(status: &AuditStatus) -> Option<&str> {
    match status {
        AuditStatus::Pass => None,
        AuditStatus::Warn(e)
        | AuditStatus::Fail(e)
        | AuditStatus::OptOut(e)
        | AuditStatus::NotApplicable(e)
        | AuditStatus::Skip(e)
        | AuditStatus::Error(e) => Some(e),
    }
}

fn evidence_mut(status: &mut AuditStatus) -> Option<&mut String> {
    match status {
        AuditStatus::Pass => None,
        AuditStatus::Warn(e)
        | AuditStatus::Fail(e)
        | AuditStatus::OptOut(e)
        | AuditStatus::NotApplicable(e)
        | AuditStatus::Skip(e)
        | AuditStatus::Error(e) => Some(e),
    }
}
