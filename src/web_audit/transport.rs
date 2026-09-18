//! The single-hop transport seam under the web audit.
//!
//! A [`Transport`] sends exactly one request and returns the response head
//! with an unread body; it never follows a redirect, so the fetch layer above
//! it sees every hop and classifies each one before connecting. The seam is
//! where the conformance corpus records exchanges, which is why the
//! production [`UreqTransport`] and the mock in [`super::mock`] share it.
//!
//! [`UreqTransport`] is ureq over rustls with the aws-lc-rs provider and the
//! bundled Mozilla roots, so a run needs no system trust store. It holds two
//! agents: one that never touches a proxy, and one carrying the configured
//! proxy, chosen per request by the caller's [`Request::via_proxy`] flag.

use std::fmt;
use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::fetch::proxy;
use super::headers::Headers;

/// One HTTP request for exactly one hop.
#[derive(Clone, Debug)]
pub struct Request {
    /// HTTP method, uppercase.
    pub method: String,
    /// Absolute URL of this hop.
    pub url: String,
    /// Request headers as the caller wrote them; names keep their casing.
    pub headers: Vec<(String, String)>,
    /// Request body, when the method carries one.
    pub body: Option<Vec<u8>>,
    /// Deadline for the whole request, response body included.
    pub timeout: Duration,
    /// Route through the configured proxy when one exists. The fetch layer
    /// sets this for public targets only; local targets always connect
    /// directly.
    pub via_proxy: bool,
}

/// A response head plus its unread body.
pub struct Response {
    /// HTTP status code.
    pub status: u16,
    /// Response headers, names lowercased.
    pub headers: Headers,
    /// The body, decoded of any content encoding, not yet read.
    pub body: Box<dyn Read>,
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .finish_non_exhaustive()
    }
}

/// Sends one request and returns one response.
pub trait Transport {
    /// Perform the request. A 3xx comes back as a response like any other.
    fn send(&self, request: &Request) -> Result<Response, TransportError>;
}

/// Why a TLS handshake failed, with the one class a user can act on singled
/// out from every other failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlsFailure {
    /// The server's chain leads to no root in the bundled Mozilla set. A
    /// private CA and a self-signed certificate both land here, and so does
    /// a public root issued after this release's bundle.
    pub bundled_root_rejection: bool,
    /// The TLS library's own account of the failure.
    pub detail: String,
}

/// A request that produced no response.
#[derive(Debug)]
pub enum TransportError {
    /// The request's deadline elapsed after the connection was made.
    Timeout,
    /// Name resolution or the connection itself timed out.
    ConnectTimeout,
    /// The TLS handshake failed.
    Tls(TlsFailure),
    /// A connection, resolution, protocol or I/O failure.
    Network(String),
    /// A failure a test double recorded verbatim.
    Verbatim(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::Timeout => f.write_str("TimeoutError: deadline exceeded"),
            TransportError::ConnectTimeout => f.write_str("TimeoutError: connect timed out"),
            TransportError::Tls(tls) if tls.bundled_root_rejection => {
                write!(
                    f,
                    "TlsError: chain rejected by bundled roots ({})",
                    tls.detail
                )
            }
            TransportError::Tls(tls) => write!(f, "TlsError: {}", tls.detail),
            TransportError::Network(detail) => write!(f, "NetworkError: {detail}"),
            TransportError::Verbatim(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<ureq::Error> for TransportError {
    fn from(err: ureq::Error) -> Self {
        match err {
            ureq::Error::Timeout(ureq::Timeout::Connect | ureq::Timeout::Resolve) => {
                TransportError::ConnectTimeout
            }
            ureq::Error::Timeout(_) => TransportError::Timeout,
            ureq::Error::Rustls(tls) => TransportError::Tls(classify_rustls(&tls)),
            ureq::Error::Tls(detail) => TransportError::Tls(TlsFailure {
                bundled_root_rejection: false,
                detail: detail.to_string(),
            }),
            // rustls reports a failed handshake through the stream as an I/O
            // error carrying its own error as the source.
            ureq::Error::Io(io) => match io
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<rustls::Error>())
            {
                Some(tls) => TransportError::Tls(classify_rustls(tls)),
                None => TransportError::Network(io.to_string()),
            },
            other => TransportError::Network(other.to_string()),
        }
    }
}

fn classify_rustls(err: &rustls::Error) -> TlsFailure {
    let bundled_root_rejection = matches!(
        err,
        rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer)
    );
    TlsFailure {
        bundled_root_rejection,
        detail: err.to_string(),
    }
}

/// The production transport: blocking ureq over rustls with bundled roots.
pub struct UreqTransport {
    direct: ureq::Agent,
    proxied: Option<ureq::Agent>,
}

impl fmt::Debug for UreqTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UreqTransport")
            .field("proxied", &self.proxied.is_some())
            .finish_non_exhaustive()
    }
}

impl UreqTransport {
    /// A transport that never uses a proxy.
    pub fn new(user_agent: &str) -> Self {
        Self::with_proxy(user_agent, None)
    }

    /// A transport whose public-path requests use the proxy named by
    /// `HTTPS_PROXY`, `HTTP_PROXY` or `ALL_PROXY`, honoring `NO_PROXY`.
    pub fn from_env(user_agent: &str) -> Self {
        Self::with_proxy(user_agent, proxy::proxy_from_env())
    }

    /// A transport with an explicit proxy for requests flagged `via_proxy`.
    pub fn with_proxy(user_agent: &str, proxy: Option<ureq::Proxy>) -> Self {
        UreqTransport {
            direct: agent(user_agent, None),
            proxied: proxy.map(|p| agent(user_agent, Some(p))),
        }
    }
}

// Redirects stay off so a 3xx returns as a response and the fetch layer
// decides whether the next hop may be contacted. Status codes are never
// errors: a 404 is evidence. The direct agent passes `None` explicitly,
// because ureq's default config reads the proxy environment on its own.
// The crypto provider is named on the agent rather than installed as the
// process default, so the choice is local to this transport.
fn agent(user_agent: &str, proxy: Option<ureq::Proxy>) -> ureq::Agent {
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::WebPki)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .build();
    ureq::Agent::config_builder()
        .user_agent(user_agent)
        .http_status_as_error(false)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .proxy(proxy)
        .tls_config(tls)
        .build()
        .new_agent()
}

/// Attempts a single probe may cost, counting the first.
///
/// A connection the pool kept but the server had already closed fails
/// before the request reaches anything, which is not a fact about the
/// target: a server that closes idle keep-alive connections, or that
/// answers HTTP/1.0 and so closes every one, would otherwise scatter
/// `error` rows across a site that is serving perfectly, and push the run
/// to the could-not-check exit code. Each failure discards the dead
/// connection, so a bounded retry walks past the stale entries the pool
/// accumulated while six threads probed at once.
const MAX_ATTEMPTS: u32 = 3;

impl Transport for UreqTransport {
    fn send(&self, request: &Request) -> Result<Response, TransportError> {
        let started = Instant::now();
        let mut attempt = 1;
        loop {
            let outcome = self.send_once(request);
            // Only a connection-level failure is retried, and only while a
            // further attempt still fits inside this request's own budget,
            // so the per-audit deadline keeps its meaning. A timeout or a
            // TLS rejection is the target's answer, not a stale socket.
            let retryable = matches!(outcome, Err(TransportError::Network(_)))
                && attempt < MAX_ATTEMPTS
                && started.elapsed() * 2 < request.timeout;
            if !retryable {
                return outcome;
            }
            attempt += 1;
        }
    }
}

impl UreqTransport {
    fn send_once(&self, request: &Request) -> Result<Response, TransportError> {
        let agent = match (&self.proxied, request.via_proxy) {
            (Some(proxied), true) => proxied,
            _ => &self.direct,
        };
        let method = ureq::http::Method::from_bytes(request.method.as_bytes())
            .map_err(|e| TransportError::Network(format!("method {:?}: {e}", request.method)))?;
        let mut builder = ureq::http::Request::builder()
            .method(method)
            .uri(request.url.as_str());
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let response = match &request.body {
            Some(body) => run(agent, builder.body(body.clone()), request.timeout),
            None => run(agent, builder.body(()), request.timeout),
        }?;
        Ok(Response::from_ureq(response))
    }
}

fn run<S: ureq::AsSendBody>(
    agent: &ureq::Agent,
    built: Result<ureq::http::Request<S>, ureq::http::Error>,
    timeout: Duration,
) -> Result<ureq::http::Response<ureq::Body>, TransportError> {
    let request = built.map_err(|e| TransportError::Network(e.to_string()))?;
    let request = agent
        .configure_request(request)
        .timeout_global(Some(timeout))
        .build();
    Ok(agent.run(request)?)
}

impl Response {
    fn from_ureq(response: ureq::http::Response<ureq::Body>) -> Self {
        let status = response.status().as_u16();
        let headers = Headers::from_pairs(
            response
                .headers()
                .iter()
                .map(|(name, value)| (name.as_str(), String::from_utf8_lossy(value.as_bytes()))),
        );
        // The reader is unbounded here on purpose: the fetch layer applies
        // the body-cap regime and the truncation flag.
        let body = response.into_body().into_reader();
        Response {
            status,
            headers,
            body: Box::new(body),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_variants_map_to_two_distinct_errors() {
        let connect = TransportError::from(ureq::Error::Timeout(ureq::Timeout::Connect));
        let resolve = TransportError::from(ureq::Error::Timeout(ureq::Timeout::Resolve));
        let global = TransportError::from(ureq::Error::Timeout(ureq::Timeout::Global));
        let body = TransportError::from(ureq::Error::Timeout(ureq::Timeout::RecvBody));
        assert!(matches!(connect, TransportError::ConnectTimeout));
        assert!(matches!(resolve, TransportError::ConnectTimeout));
        assert!(matches!(global, TransportError::Timeout));
        assert!(matches!(body, TransportError::Timeout));
        assert_eq!(global.to_string(), "TimeoutError: deadline exceeded");
    }

    #[test]
    fn unknown_issuer_is_the_bundled_root_class_and_other_tls_errors_are_not() {
        let unknown = ureq::Error::Rustls(rustls::Error::InvalidCertificate(
            rustls::CertificateError::UnknownIssuer,
        ));
        match TransportError::from(unknown) {
            TransportError::Tls(tls) => assert!(tls.bundled_root_rejection),
            other => panic!("{other:?}"),
        }
        let expired = ureq::Error::Rustls(rustls::Error::InvalidCertificate(
            rustls::CertificateError::Expired,
        ));
        match TransportError::from(expired) {
            TransportError::Tls(tls) => {
                assert!(!tls.bundled_root_rejection);
                assert!(tls.detail.contains("xpired"), "{}", tls.detail);
            }
            other => panic!("{other:?}"),
        }
        let generic = TransportError::from(ureq::Error::Tls("handshake refused"));
        assert_eq!(generic.to_string(), "TlsError: handshake refused");
    }

    #[test]
    fn a_rustls_error_wrapped_in_io_is_still_classified() {
        let inner = rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer);
        let io = std::io::Error::new(std::io::ErrorKind::InvalidData, inner);
        let err = TransportError::from(ureq::Error::Io(io));
        assert!(
            err.to_string().contains("chain rejected by bundled roots"),
            "{err}"
        );
    }

    #[test]
    fn network_and_verbatim_render_distinctly() {
        let refused = TransportError::from(ureq::Error::ConnectionFailed);
        assert_eq!(refused.to_string(), "NetworkError: connection failed");
        let verbatim = TransportError::Verbatim("TimeoutError: deadline exceeded".into());
        assert_eq!(verbatim.to_string(), "TimeoutError: deadline exceeded");
    }
}
