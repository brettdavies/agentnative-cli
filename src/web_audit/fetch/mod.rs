//! The guarded fetch above the transport seam, a port of the site engine's
//! `guardedFetch`.
//!
//! A fetch never fails as an error: every outcome is a [`ProbeResponse`],
//! either a status with headers and a capped body, or a `status: None` row
//! carrying the reason in `error`. The layer injects the audit User-Agent
//! when the caller set none, follows redirects through [`redirect`] so every
//! hop is classified first, and reads the body under the regime in [`body`].

pub mod body;
pub mod proxy;
pub mod redirect;

use std::time::{Duration, Instant};

use super::headers::Headers;
use super::locality::Resolver;
use super::transport::{Response, Transport, TransportError};

/// Per-request deadline when the caller sets none.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(8_000);

/// The request a caller wants sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchInit {
    /// HTTP method, uppercase.
    pub method: String,
    /// Request headers as written; a `User-Agent` here replaces the audit's.
    pub headers: Vec<(String, String)>,
    /// Request body.
    pub body: Option<Vec<u8>>,
}

impl Default for FetchInit {
    fn default() -> Self {
        FetchInit {
            method: "GET".to_string(),
            headers: Vec::new(),
            body: None,
        }
    }
}

/// How the chain and body are handled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchOptions {
    /// Deadline for each hop.
    pub timeout: Duration,
    /// Body cap: `Some(0)` skips the body, `Some(n)` keeps `n` bytes, `None`
    /// reads up to the root ceiling.
    pub max_body_bytes: Option<usize>,
    /// Hops followed before the chain is abandoned.
    pub max_redirects: usize,
    /// When false a 3xx is returned as-is with its `Location` header.
    pub follow_redirects: bool,
}

impl Default for FetchOptions {
    fn default() -> Self {
        FetchOptions {
            timeout: DEFAULT_TIMEOUT,
            max_body_bytes: None,
            max_redirects: redirect::DEFAULT_MAX_REDIRECTS,
            follow_redirects: true,
        }
    }
}

/// What a probe observed, mirroring the site's `ProbeResponse`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeResponse {
    /// HTTP status, or `None` when no response was produced.
    pub status: Option<u16>,
    /// Response headers, names lowercased.
    pub headers: Headers,
    /// The body as text, lossily decoded from UTF-8.
    pub body: String,
    /// The reason no response was produced.
    pub error: Option<String>,
    /// Wall-clock milliseconds the whole chain took.
    pub elapsed_ms: u64,
    /// The body was cut at its cap.
    pub truncated: bool,
}

impl ProbeResponse {
    /// A fetch that produced no response.
    pub fn failed(error: String, elapsed_ms: u64) -> Self {
        ProbeResponse {
            status: None,
            headers: Headers::new(),
            body: String::new(),
            error: Some(error),
            elapsed_ms,
            truncated: false,
        }
    }
}

/// The guarded fetch bound to one transport, one resolver and one identity.
#[derive(Clone, Copy)]
pub struct Fetcher<'a> {
    transport: &'a dyn Transport,
    resolver: &'a dyn Resolver,
    user_agent: &'a str,
}

impl std::fmt::Debug for Fetcher<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fetcher")
            .field("user_agent", &self.user_agent)
            .finish_non_exhaustive()
    }
}

impl<'a> Fetcher<'a> {
    /// Bind the fetch layer to its transport, resolver and User-Agent.
    pub fn new(
        transport: &'a dyn Transport,
        resolver: &'a dyn Resolver,
        user_agent: &'a str,
    ) -> Self {
        Fetcher {
            transport,
            resolver,
            user_agent,
        }
    }

    /// The audit identity sent when a request carries no User-Agent.
    pub fn user_agent(&self) -> &'a str {
        self.user_agent
    }

    /// Fetch through the guard. Never returns an error; failures come back
    /// as a response with `status: None`.
    pub fn fetch(&self, url: &str, init: &FetchInit, opts: &FetchOptions) -> ProbeResponse {
        let started = Instant::now();
        let elapsed =
            |started: Instant| u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

        let target = match redirect::validate_target(url, self.resolver) {
            Ok(target) => target,
            Err(reason) => return ProbeResponse::failed(reason, elapsed(started)),
        };
        let mut headers = init.headers.clone();
        if !has_header(&headers, "user-agent") {
            headers.push(("user-agent".to_string(), self.user_agent.to_string()));
        }
        let chain = redirect::ChainRequest {
            transport: self.transport,
            resolver: self.resolver,
            method: &init.method,
            headers: &headers,
            body: init.body.as_deref(),
            timeout: opts.timeout,
            follow_redirects: opts.follow_redirects,
            max_redirects: opts.max_redirects,
        };
        let Response {
            status,
            headers,
            body,
        } = match redirect::follow(&chain, target) {
            Ok(response) => response,
            Err(reason) => return ProbeResponse::failed(reason, elapsed(started)),
        };
        let read = match body::read_capped(body, opts.max_body_bytes) {
            Ok(read) => read,
            Err(e) => {
                let reason = TransportError::from(ureq::Error::from(e)).to_string();
                return ProbeResponse::failed(reason, elapsed(started));
            }
        };
        ProbeResponse {
            status: Some(status),
            headers,
            body: String::from_utf8_lossy(&read.bytes).into_owned(),
            error: None,
            elapsed_ms: elapsed(started),
            truncated: read.truncated,
        }
    }
}

fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
}
