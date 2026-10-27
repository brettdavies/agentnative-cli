//! One redirect hop's answer and the rules for reusing it, a port of the
//! site engine's `request-hop.ts`.
//!
//! A hop's answer has two stages: the status and headers, then the body read
//! as far as the caller needs. Without a request memo a hop goes straight
//! through the transport; with one, the [`super::memo::RequestMemo`] answers
//! from a record whenever that record serves the caller as its own request
//! would have, and [`body_of`] gives the caller its share of the body.

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::body;
use super::redirect::is_redirect;
use crate::web_audit::headers::Headers;
use crate::web_audit::transport::{Request, Transport, TransportError};

/// The message every deadline that passes before an answer arrives carries.
pub const DEADLINE_EXCEEDED: &str = "TimeoutError: deadline exceeded";

/// What one caller needs from a hop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HopContext {
    /// When this hop began; the caller's budget for it runs from here.
    pub started: Instant,
    /// When the caller's deadline passes.
    pub deadline: Instant,
    /// The caller's cap for a body it keeps: `Some(0)` reads none, `None`
    /// reads up to the root ceiling.
    pub body_cap: Option<usize>,
    /// The caller may keep a redirect as its answer instead of following it,
    /// so it reads the redirect's body.
    pub keeps_redirect: bool,
}

impl HopContext {
    /// The caller's whole budget for this hop.
    fn budget(&self) -> Duration {
        self.deadline.saturating_duration_since(self.started)
    }
}

/// Why a request or its body read got no further, and when.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HopError {
    /// The reason, as the response's `error` carries it.
    pub message: String,
    /// The request or read ran out of time rather than failing.
    pub timed_out: bool,
    pub(crate) started: Instant,
    pub(crate) ended: Instant,
    pub(crate) deadline: Instant,
}

/// A response the target sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HopAnswer {
    /// HTTP status.
    pub status: u16,
    /// Response headers, names lowercased.
    pub headers: Headers,
    pub(crate) started: Instant,
    pub(crate) headers_after: Duration,
    pub(crate) bytes: Arc<Vec<u8>>,
    /// The cap the body was read with: `None` = to the root ceiling, `Some(0)` = not read.
    pub(crate) body_cap: Option<usize>,
    pub(crate) truncated: bool,
    pub(crate) elapsed: Duration,
    /// Set when the body read failed after the status and headers arrived.
    pub body_error: Option<HopError>,
}

/// A hop's outcome: a response, or the reason none came.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HopRecord {
    /// The target answered.
    Answer(HopAnswer),
    /// No response.
    Failure(HopError),
}

/// The body as one caller reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyRead {
    /// The bytes within the caller's cap.
    pub bytes: Vec<u8>,
    /// The body continued past what the caller reads.
    pub truncated: bool,
}

impl HopAnswer {
    /// The status and headers, with no body read yet.
    pub(crate) fn head(status: u16, headers: Headers, started: Instant) -> Self {
        let headers_after = started.elapsed();
        HopAnswer {
            status,
            headers,
            started,
            headers_after,
            bytes: Arc::new(Vec::new()),
            body_cap: Some(body::STATUS_ONLY_BODY_BYTES),
            truncated: false,
            elapsed: headers_after,
            body_error: None,
        }
    }

    /// This answer with the body a read produced.
    pub(crate) fn with_body(&self, read: body::BodyRead, cap: Option<usize>) -> Self {
        HopAnswer {
            bytes: Arc::new(read.bytes),
            body_cap: cap,
            truncated: read.truncated,
            elapsed: self.started.elapsed(),
            ..self.clone()
        }
    }

    /// This answer with a body read that failed.
    pub(crate) fn with_body_error(&self, error: HopError) -> Self {
        HopAnswer {
            elapsed: error.ended.saturating_duration_since(self.started),
            body_error: Some(error),
            ..self.clone()
        }
    }

    /// The body's bytes on record.
    pub(crate) fn body_len(&self) -> usize {
        self.bytes.len()
    }

    /// This answer with its body let go.
    pub(crate) fn without_body(&self) -> Self {
        HopAnswer {
            bytes: Arc::new(Vec::new()),
            body_cap: Some(body::STATUS_ONLY_BODY_BYTES),
            truncated: false,
            ..self.clone()
        }
    }
}

impl HopError {
    /// A failure observed now.
    pub(crate) fn new(message: String, timed_out: bool, ctx: &HopContext) -> Self {
        HopError {
            message,
            timed_out,
            started: ctx.started,
            ended: Instant::now(),
            deadline: ctx.deadline,
        }
    }

    /// A deadline that passed at `at`.
    pub(crate) fn timeout_at(started: Instant, at: Instant) -> Self {
        HopError {
            message: DEADLINE_EXCEEDED.to_string(),
            timed_out: true,
            started,
            ended: at,
            deadline: at,
        }
    }

    /// The transport's account of a request that got no response.
    pub(crate) fn from_transport(err: &TransportError, ctx: &HopContext) -> Self {
        let timed_out = match err {
            TransportError::Timeout | TransportError::ConnectTimeout => true,
            TransportError::Verbatim(message) => {
                message.starts_with("TimeoutError") || message.starts_with("AbortError")
            }
            _ => false,
        };
        HopError::new(err.to_string(), timed_out, ctx)
    }

    /// A body read that failed partway.
    pub(crate) fn from_body(err: std::io::Error, ctx: &HopContext) -> Self {
        if body::DeadlinePassed::is(&err) {
            return HopError::new(DEADLINE_EXCEEDED.to_string(), true, ctx);
        }
        let message = TransportError::from(ureq::Error::from(err)).to_string();
        let timed_out = message.starts_with("TimeoutError");
        HopError::new(message, timed_out, ctx)
    }
}

/// The caller ran out of time waiting for an answer.
pub(crate) fn timed_out_for(ctx: &HopContext) -> HopRecord {
    HopRecord::Failure(HopError::new(DEADLINE_EXCEEDED.to_string(), true, ctx))
}

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

/// Send one hop straight through the transport. A caller that reads the
/// body reads it here, checking its deadline between chunks; a caller that
/// skips it drops the body unread.
pub(crate) fn take(transport: &dyn Transport, request: &Request, ctx: &HopContext) -> HopRecord {
    let response = match transport.send(request) {
        Ok(response) => response,
        Err(err) => return HopRecord::Failure(HopError::from_transport(&err, ctx)),
    };
    let head = HopAnswer::head(response.status, response.headers, ctx.started);
    if !reads_body(head.status, &head.headers, ctx) {
        drop(response.body);
        return HopRecord::Answer(head);
    }
    let deadline = ctx.deadline;
    match body::read_capped_until(response.body, ctx.body_cap, || Instant::now() < deadline) {
        Ok(read) => HopRecord::Answer(head.with_body(read, ctx.body_cap)),
        Err(err) => HopRecord::Answer(head.with_body_error(HopError::from_body(err, ctx))),
    }
}
