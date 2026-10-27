//! Artifact reads outside the follow slice, a mirror of the site's
//! `directArtifactSource`: discovery reads a challenged common path's
//! metadata through it, and the engine reads the endpoint of record's.

use std::time::{Duration, Instant};

use super::follow_requests::{Cause, ReadOptions};
use super::reciprocity_metadata::ArtifactSource;
use super::types::FetchHandle;
use crate::web_audit::fetch::{FetchInit, FetchOptions, ProbeResponse};

/// One GET per location, redirects disabled, each under the per-request
/// timeout bounded by `deadline_at`. Once the deadline passes nothing more
/// is sent.
#[derive(Clone, Debug)]
pub struct DirectSource {
    /// The run's transport and resolver.
    pub fetch: FetchHandle,
    /// The per-request timeout.
    pub timeout: Duration,
    /// When reads stop.
    pub deadline_at: Instant,
}

impl ArtifactSource for DirectSource {
    fn get(&mut self, url: &str, opts: ReadOptions) -> Result<ProbeResponse, Cause> {
        let left = self
            .timeout
            .min(self.deadline_at.saturating_duration_since(Instant::now()));
        if left.is_zero() {
            return Ok(ProbeResponse::failed("budget spent".to_string(), 0));
        }
        let headers = opts
            .accept
            .map(|accept| vec![("accept".to_string(), accept.to_string())])
            .unwrap_or_default();
        let init = FetchInit {
            headers,
            ..FetchInit::default()
        };
        let fetch_opts = FetchOptions {
            timeout: left,
            max_body_bytes: Some(opts.max_body_bytes),
            follow_redirects: false,
            ..FetchOptions::default()
        };
        Ok(self.fetch.fetch(url, &init, &fetch_opts))
    }

    fn admits(&self, url: &str) -> bool {
        self.fetch.validate(url).is_ok()
    }
}
