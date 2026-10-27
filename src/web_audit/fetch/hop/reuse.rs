//! Whether an answer on record serves a caller as its own request would
//! have, and how much of the body it can give that caller.

use std::time::{Duration, Instant};

use super::{BodyRead, HopAnswer, HopContext, HopError, HopRecord};
use crate::web_audit::fetch::body;
use crate::web_audit::fetch::redirect::is_redirect;
use crate::web_audit::headers::Headers;

/// Whether the caller reads a body from this answer: not for a redirect it
/// follows, and not for a status-only read.
pub(crate) fn reads_body(status: u16, headers: &Headers, ctx: &HopContext) -> bool {
    let followed = is_redirect(status) && headers.contains("location") && !ctx.keeps_redirect;
    ctx.body_cap != Some(body::STATUS_ONLY_BODY_BYTES) && !followed
}

/// The body as a caller with `cap` reads it, or `None` when the bytes on
/// record cannot serve that cap.
pub fn body_of(answer: &HopAnswer, cap: Option<usize>) -> Option<BodyRead> {
    if cap == Some(body::STATUS_ONLY_BODY_BYTES) {
        return Some(BodyRead {
            bytes: Vec::new(),
            truncated: false,
        });
    }
    if answer.body_cap == Some(body::STATUS_ONLY_BODY_BYTES) {
        return None;
    }
    if answer.truncated && effective_cap(cap) > effective_cap(answer.body_cap) {
        return None;
    }
    let kept = cap.map_or(answer.bytes.len(), |c| c.min(answer.bytes.len()));
    Some(BodyRead {
        bytes: answer.bytes[..kept].to_vec(),
        truncated: answer.truncated || kept < answer.bytes.len(),
    })
}

/// Whether something that took `took` and arrived at `arrived` reaches the
/// caller as its own request would have. One that settled while the caller
/// waited reached it by its deadline; one already on record fits when the
/// time it took fits the caller's budget.
fn in_time(took: Duration, arrived: Instant, waited: bool, ctx: &HopContext) -> bool {
    if waited {
        arrived <= ctx.deadline
    } else {
        took <= ctx.budget()
    }
}

/// Whether a recorded error answers the caller as its own request would
/// have. A timeout already on record answers a caller whose budget is no
/// longer than the one that ran out; a timeout that ran out while the caller
/// waited ended before the caller's deadline, so its own request still has
/// time. Any other error answers when it arrived in time.
fn error_answers(error: &HopError, waited: bool, ctx: &HopContext) -> bool {
    if !error.timed_out {
        let took = error.ended.saturating_duration_since(error.started);
        return in_time(took, error.ended, waited, ctx);
    }
    !waited && ctx.budget() <= error.deadline.saturating_duration_since(error.started)
}

/// Whether a stored record answers a caller as a request of its own would have.
pub(crate) fn answers(record: &HopRecord, waited: bool, ctx: &HopContext) -> bool {
    let answer = match record {
        HopRecord::Failure(error) => return error_answers(error, waited, ctx),
        HopRecord::Answer(answer) => answer,
    };
    if !reads_body(answer.status, &answer.headers, ctx) {
        let arrived = answer.started + answer.headers_after;
        return in_time(answer.headers_after, arrived, waited, ctx);
    }
    if let Some(error) = &answer.body_error {
        return error_answers(error, waited, ctx);
    }
    let arrived = answer.started + answer.elapsed;
    in_time(answer.elapsed, arrived, waited, ctx) && body_of(answer, ctx.body_cap).is_some()
}

/// How much of the body a record can serve: nothing (a failure), up to its
/// cap, or all of it.
pub(crate) fn coverage(record: &HopRecord) -> i128 {
    match record {
        HopRecord::Failure(_) => -1,
        HopRecord::Answer(answer) => match answer.body_cap {
            Some(body::STATUS_ONLY_BODY_BYTES) => 0,
            _ if !answer.truncated => i128::MAX,
            cap => effective_cap(cap) as i128,
        },
    }
}

/// The bytes a cap reads at most. An uncapped read stops at the root
/// ceiling, where the site's reads in full; a body cut there serves every
/// uncapped reader the same cut.
fn effective_cap(cap: Option<usize>) -> usize {
    cap.unwrap_or(body::AUDIT_ROOT_MAX_BODY_BYTES)
}
