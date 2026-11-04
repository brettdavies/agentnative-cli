//! One check's probe and the rows a check can finalize to.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use super::handlers::{Dispatch, HandlerSet};
use super::summary::summarize_evidence;
use super::types::{HandlerContext, ProbeOutcome, why_item};
use crate::web_audit::locality::Locality;
use crate::web_audit::registry::{HandlerBinding, WebCheck, WebCheckKeyword};
use crate::web_audit::scorecard::{EngineResult, EvidenceItem, NaReason, ScorecardStatus};

/// The evidence line on a DNS row the locality gate withheld.
pub const EXTERNAL_DNS_WITHHELD: &str = "needs public DNS: verified on anc.dev";

pub(super) fn to_result(check: &'static WebCheck, outcome: ProbeOutcome) -> EngineResult {
    EngineResult {
        check,
        status: outcome.status.to_scorecard(),
        na_reason: outcome.na_reason,
        unprobed: outcome.unprobed,
        advisory: outcome.advisory,
        evidence: summarize_evidence(check, &outcome),
        raw_evidence: outcome.evidence,
    }
}

pub(super) fn na_result(
    check: &'static WebCheck,
    reason: NaReason,
    evidence: &str,
) -> EngineResult {
    EngineResult {
        check,
        status: ScorecardStatus::NA,
        na_reason: Some(reason),
        unprobed: false,
        advisory: None,
        evidence: evidence.to_string(),
        raw_evidence: vec![why_item(evidence)],
    }
}

/// An `n_a` row that reads as the host it could not be evaluated at rather
/// than the audited one.
pub(super) fn na_at_host(mut result: EngineResult, host: Option<String>) -> EngineResult {
    if let Some(host) = host {
        for item in &mut result.raw_evidence {
            item.insert("host".to_string(), serde_json::Value::String(host.clone()));
        }
    }
    result
}

pub(super) fn error_result(check: &'static WebCheck, message: &str) -> EngineResult {
    let mut item = EvidenceItem::new();
    item.insert(
        "error".to_string(),
        serde_json::Value::String(message.to_string()),
    );
    EngineResult {
        check,
        status: ScorecardStatus::Error,
        na_reason: None,
        unprobed: false,
        advisory: None,
        evidence: message.to_string(),
        raw_evidence: vec![item],
    }
}

fn skip_result(check: &'static WebCheck, why: &str, evidence: &str) -> EngineResult {
    EngineResult {
        check,
        status: ScorecardStatus::Skip,
        na_reason: None,
        unprobed: false,
        advisory: None,
        evidence: evidence.to_string(),
        raw_evidence: vec![why_item(why)],
    }
}

pub(super) fn deadline_skip(check: &'static WebCheck) -> EngineResult {
    skip_result(
        check,
        "per-audit deadline exceeded",
        "skipped: per-audit deadline exceeded",
    )
}

fn unported_skip(check: &'static WebCheck, kind: &str) -> EngineResult {
    let why = format!("handler `{kind}` not yet ported to the local engine");
    skip_result(check, &why, &why)
}

/// An applicable MAY that is simply absent is optional, not a miss.
pub(super) fn finalize_optional(check: &WebCheck, mut result: EngineResult) -> EngineResult {
    if check.keyword == WebCheckKeyword::May && result.status == ScorecardStatus::Absent {
        result.status = ScorecardStatus::NA;
        result.na_reason = Some(NaReason::OptionalAbsent);
    }
    result
}

/// What one probe left behind.
pub(super) struct Probed {
    pub(super) check: &'static WebCheck,
    pub(super) outcome: Option<ProbeOutcome>,
    pub(super) result: EngineResult,
    pub(super) incomplete: bool,
    pub(super) unported: Option<&'static str>,
}

/// Run one check's handler, or skip it past the deadline. A handler's panic
/// is that row's error and goes no further.
pub(super) fn probe_one(
    check: &'static WebCheck,
    ctx: &HandlerContext,
    handlers: &HandlerSet,
) -> Probed {
    let deadline = ctx.deadline;
    let local_target = ctx.target_locality == Some(Locality::Local);
    let done = |result: EngineResult, incomplete: bool| Probed {
        check,
        outcome: None,
        result,
        incomplete,
        unported: None,
    };
    if Instant::now() >= deadline {
        return done(deadline_skip(check), true);
    }
    if check.handler == HandlerBinding::DnsDoh && local_target && !ctx.external_dns {
        return done(
            EngineResult {
                check,
                status: ScorecardStatus::NA,
                na_reason: None,
                unprobed: false,
                advisory: None,
                evidence: EXTERNAL_DNS_WITHHELD.to_string(),
                raw_evidence: vec![why_item(EXTERNAL_DNS_WITHHELD)],
            },
            false,
        );
    }
    let dispatched = catch_unwind(AssertUnwindSafe(|| match handlers.dispatch(check, ctx) {
        Dispatch::Outcome(outcome) => Ok((to_result(check, outcome.clone()), outcome)),
        Dispatch::Unported(kind) => Err(kind),
    }));
    match dispatched {
        Ok(Ok((result, outcome))) => Probed {
            check,
            result,
            incomplete: outcome.incomplete || Instant::now() >= deadline,
            outcome: Some(outcome),
            unported: None,
        },
        Ok(Err(kind)) => Probed {
            check,
            outcome: None,
            result: unported_skip(check, kind),
            incomplete: false,
            unported: Some(kind),
        },
        Err(panic) => {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "handler panicked".to_string());
            done(
                error_result(check, &format!("handler error: {message}")),
                false,
            )
        }
    }
}
