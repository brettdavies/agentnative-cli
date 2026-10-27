//! How the `mcp` handler reads a 401: a row that needs a token the auditor
//! does not hold reads `n_a` `auth-required` once the endpoint's own RFC
//! 9728 metadata backs the 401, and the enforcement row, which asks whether
//! a request without a token is refused, reads the 401 as its answer.

use serde_json::{Map, Value, json};

use super::ops::Era;
use crate::web_audit::engine::sign_in::SignInChallenge;
use crate::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus};
use crate::web_audit::scorecard::{EvidenceItem, NaReason};

fn with_why(mut ev: EvidenceItem, line: &str) -> EvidenceItem {
    ev.insert("why".to_string(), json!([line]));
    ev
}

/// Whether a request carrying no token was refused with the 401 that tells
/// a client how to sign in. Any other answer fails the row without
/// misleading the caller, who got either what it asked for or a refusal,
/// so it is noncompliant rather than broken: serving some methods without
/// a token and challenging the rest is a design, not a trap.
pub fn enforcement_verdict(
    status: Option<u16>,
    rpc: Option<&Map<String, Value>>,
    ev: EvidenceItem,
) -> ProbeOutcome {
    if status == Some(401) {
        return ProbeOutcome::new(ProbeStatus::Pass, vec![with_why(ev, "refused with 401")]);
    }
    let why = if rpc.is_some_and(|r| r.contains_key("result")) {
        "a request without a token was served a result".to_string()
    } else {
        let status = status.map_or("null".to_string(), |s| s.to_string());
        format!("a request without a token was answered with HTTP {status} instead of 401")
    };
    ProbeOutcome::new(ProbeStatus::Noncompliant, vec![with_why(ev, &why)])
}

/// A 401 from an endpoint that requires sign-in: the row needs a token the
/// auditor does not hold.
fn sign_in_required(ev: &EvidenceItem) -> ProbeOutcome {
    let url = match ev.get("url") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => "undefined".to_string(),
    };
    ProbeOutcome {
        na_reason: Some(NaReason::AuthRequired),
        ..ProbeOutcome::new(
            ProbeStatus::Na,
            vec![with_why(ev.clone(), &format!("HTTP 401 from {url}"))],
        )
    }
}

/// A wave-1 outcome read again once the endpoint is known to require
/// sign-in, through the arm every later row takes; `None` when that arm
/// would not reach it (a failed request or a rate-limit refusal).
pub fn sign_in_required_outcome(outcome: &ProbeOutcome) -> Option<ProbeOutcome> {
    let first = outcome.evidence.first()?;
    let drew_401 = first.get("status").and_then(Value::as_u64) == Some(401);
    (outcome.status != ProbeStatus::Error && drew_401).then(|| sign_in_required(first))
}

/// The row's 401 asks for sign-in: wave 1 settled that the endpoint
/// requires it, or this 401 is the first that metadata naming the endpoint
/// backs. Settled ahead of every arm that reads the answer, which would
/// read it as a broken surface or as an era the server does not serve.
pub fn signed_in_answer(
    ctx: &HandlerContext,
    status: Option<u16>,
    challenge: Option<&str>,
    lane: Era,
    ev: &EvidenceItem,
) -> Option<ProbeOutcome> {
    if status != Some(401) {
        return None;
    }
    let backed = ctx.mcp_auth.is_some()
        || ctx.mcp_sign_in.as_ref().is_some_and(|resolver| {
            resolver
                .resolve(&SignInChallenge {
                    challenge: challenge.map(str::to_string),
                    lane,
                })
                .is_some()
        });
    backed.then(|| sign_in_required(ev))
}
