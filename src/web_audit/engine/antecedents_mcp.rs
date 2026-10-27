//! MCP antecedents, a mirror of the site's `antecedents/mcp.ts`: whether an
//! MCP endpoint was discovered, whether it challenges for auth or requires
//! sign-in, and whether the rows that need a session can have one. With no
//! endpoint, a declared host that was not evaluated names why.

use super::antecedents::{AntecedentContext, Resolution};
use super::mcp_wire::advertises_resources;
use super::sign_in::{lane_refused, wire_probe_evidence, wire_probe_served};
use crate::web_audit::handlers::mcp::ops::Era;
use crate::web_audit::scorecard::EvidenceItem;

/// The evidence line of an MCP row with no endpoint to probe.
pub const NO_ENDPOINT: &str = "no MCP endpoint discovered";

/// `n_a` for a check that needs the MCP endpoint there is none of, naming
/// the declared host behind that when one is.
pub(super) fn no_mcp_endpoint(ctx: &AntecedentContext<'_>) -> Resolution {
    ctx.follow_unmet.map_or(Resolution::NotApplicable, |unmet| {
        Resolution::DeclaredHost(unmet.clone())
    })
}

/// A 401 or `WWW-Authenticate` challenge in a probe's evidence.
pub(super) fn evidence_shows_auth_challenge<'a>(
    items: impl IntoIterator<Item = &'a EvidenceItem>,
) -> bool {
    items.into_iter().any(|item| {
        item.get("status").and_then(|s| s.as_u64()) == Some(401)
            || item.get("www_authenticate").is_some_and(|v| v.is_string())
    })
}

/// A 401 or challenge on either wave-1 MCP handshake, legacy or modern, as
/// a client without a token was answered.
pub(super) fn handshake_shows_auth_challenge(ctx: &AntecedentContext<'_>) -> bool {
    evidence_shows_auth_challenge(wire_probe_evidence(ctx.token_less))
}

pub(super) fn card_declares_auth(ctx: &AntecedentContext<'_>) -> bool {
    ctx.discovery_evidence
        .iter()
        .any(|item| item.get("authentication") == Some(&serde_json::Value::Bool(true)))
}

/// A row that needs a session asks for something only a signed-in client
/// receives, so on an endpoint that requires sign-in it is not evaluated
/// rather than read as absent or broken, unless the run's own handshake was
/// served: without a token, the endpoint serves clients that hold none;
/// with one, the token opened it. A handshake answered with a JSON-RPC
/// error is a lane refusing the request, not serving it, so only a result
/// shows that. A lane whose own handshake was refused that way is not one
/// sign-in blocks: its rows run and read as they do on an open server.
fn sign_in_blocks(ctx: &AntecedentContext<'_>, lane: Option<Era>) -> bool {
    if ctx.mcp_auth.is_none() || wire_probe_served(ctx.sources) {
        return false;
    }
    lane.is_none_or(|lane| !lane_refused(ctx.sources, lane))
}

pub(super) fn mcp_auth(ctx: &AntecedentContext<'_>) -> Resolution {
    if ctx.mcp_endpoint.is_none() {
        return no_mcp_endpoint(ctx);
    }
    apply_if(handshake_shows_auth_challenge(ctx) || card_declares_auth(ctx))
}

pub(super) fn mcp_session(ctx: &AntecedentContext<'_>) -> Resolution {
    let Some(endpoint) = ctx.mcp_endpoint else {
        return no_mcp_endpoint(ctx);
    };
    if sign_in_blocks(ctx, ctx.mcp_lane) {
        Resolution::AuthRequired(endpoint.to_string())
    } else {
        Resolution::Apply
    }
}

/// Holds only on the endpoint's own answer, a 401 its RFC 9728 metadata
/// backs, so an open server reads its sign-in rows not applicable even when
/// its card documents authentication.
pub(super) fn mcp_auth_required(ctx: &AntecedentContext<'_>) -> Resolution {
    if ctx.mcp_endpoint.is_none() {
        return match no_mcp_endpoint(ctx) {
            Resolution::NotApplicable => Resolution::NotApplicableBecause(NO_ENDPOINT),
            unmet => unmet,
        };
    }
    apply_if(ctx.mcp_auth.is_some())
}

/// Era-neutral: either handshake's capability advertisement satisfies it.
/// Reading resources needs a session too, and an endpoint that requires
/// sign-in advertises nothing to a client without one, so a handshake that
/// sign-in blocked leaves unread whether the row applies.
pub(super) fn mcp_resources(ctx: &AntecedentContext<'_>) -> Resolution {
    let session = mcp_session(ctx);
    let (Resolution::Apply, Some(endpoint)) = (&session, ctx.mcp_endpoint) else {
        return session;
    };
    let evidence = |id: &str| ctx.sources.get(id).map_or(&[][..], |o| &o.evidence[..]);
    if advertises_resources(evidence("mcp-initialize"))
        || advertises_resources(evidence("mcp-server-discover"))
    {
        return Resolution::Apply;
    }
    if sign_in_blocks(ctx, Some(Era::Legacy)) || sign_in_blocks(ctx, Some(Era::Modern)) {
        Resolution::AuthRequired(endpoint.to_string())
    } else {
        Resolution::NotApplicable
    }
}

fn apply_if(holds: bool) -> Resolution {
    if holds {
        Resolution::Apply
    } else {
        Resolution::NotApplicable
    }
}
