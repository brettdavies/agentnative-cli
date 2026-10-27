//! One redirect hop's answer and the rules for reusing it, a port of the
//! site engine's `request-hop.ts`.
//!
//! A hop's answer has two stages: the status and headers, then the body read
//! as far as the caller needs. Without a request memo a hop goes straight
//! through the transport; with one, the [`super::memo::RequestMemo`] answers
//! from a record whenever that record serves the caller as its own request
//! would have, and [`body_of`] gives the caller its share of the body. The
//! records live here; the rules that decide whether one serves a caller live
//! in `reuse`.

mod reuse;

use std::sync::Arc;
use std::time::{Duration, Instant};

use super::body;
use crate::web_audit::headers::Headers;
use crate::web_audit::transport::{Request, Transport, TransportError};
pub use reuse::body_of;
pub(crate) use reuse::{answers, coverage, reads_body};

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
