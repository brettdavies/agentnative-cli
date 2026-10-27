//! The single-hop transport seam under the web audit.
//!
//! A [`Transport`] sends exactly one request and returns the response head
//! with an unread body; it never follows a redirect, so the fetch layer above
//! it sees every hop and classifies each one before connecting. The seam is
//! where the conformance corpus records exchanges, which is why the
//! production [`UreqTransport`] and the mock in [`super::mock`] share it.
//!
//! [`UreqTransport`] is ureq over rustls with the aws-lc-rs provider. A
//! public request verifies against the bundled Mozilla roots plus any
//! `--ca-cert` roots, so it needs no system trust store; a local request
//! also trusts the OS store ([`super::fetch::trust`]). It holds three
//! agents: a public one that never touches a proxy, a public one carrying
//! the configured proxy, chosen by [`Request::via_proxy`], and a local one,
//! chosen by [`Request::trust`] and built the first time a local request
//! needs it, so a public run never reads the OS store.

use std::fmt;
use std::io::Read;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use super::fetch::proxy;
use super::fetch::trust::{TrustClass, TrustConfig};
use super::headers::Headers;
use super::tls;

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
    /// The trust set the handshake verifies against, from the hop's
    /// locality class.
    pub trust: TrustClass,
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
    /// The server's chain leads to no root in the trust set. A private CA
    /// absent from the set and a self-signed certificate both land here,
    /// and so does a public root issued after this release's bundle.
    pub root_rejection: bool,
    /// The trust set the chain was checked against, when the transport
    /// knows it.
    pub trust_set: Option<&'static str>,
    /// The TLS library's own account of the failure.
    pub detail: String,
}

/// How a root rejection's evidence begins, the one string a caller can
/// recognize the class by once the error is text.
pub const ROOT_REJECTION_PREFIX: &str = "TlsError: chain rejected by ";

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
            TransportError::Tls(tls) if tls.root_rejection => write!(
                f,
                "{ROOT_REJECTION_PREFIX}{} ({})",
                tls.trust_set.unwrap_or("the trust set"),
                tls.detail
            ),
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
                root_rejection: false,
                trust_set: None,
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
    let root_rejection = matches!(
        err,
        rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer)
    );
    TlsFailure {
        root_rejection,
        trust_set: None,
        detail: err.to_string(),
    }
}

impl TransportError {
    /// Name the trust set a TLS failure was checked against.
    fn checked_against(self, trust_set: &'static str) -> Self {
        match self {
            TransportError::Tls(tls) => TransportError::Tls(TlsFailure {
                trust_set: Some(trust_set),
                ..tls
            }),
            other => other,
        }
    }
}

/// The production transport: blocking ureq over rustls, one agent per
/// trust class and proxy route.
pub struct UreqTransport {
    user_agent: String,
    trust: TrustConfig,
    direct: ureq::Agent,
    proxied: Option<ureq::Agent>,
    local: OnceLock<ureq::Agent>,
}

impl fmt::Debug for UreqTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UreqTransport")
            .field("proxied", &self.proxied.is_some())
            .field("trust", &self.trust)
            .finish_non_exhaustive()
    }
}

impl UreqTransport {
    /// A transport that never uses a proxy and trusts the default set.
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
        Self::with_trust(user_agent, proxy, TrustConfig::default())
    }

    /// A transport with an explicit proxy and trust configuration.
    pub fn with_trust(user_agent: &str, proxy: Option<ureq::Proxy>, trust: TrustConfig) -> Self {
        let public = tls::client_config(&trust, TrustClass::Public);
        UreqTransport {
            user_agent: user_agent.to_string(),
            direct: agent(user_agent, None, public.clone()),
            proxied: proxy.map(|p| agent(user_agent, Some(p), public)),
            local: OnceLock::new(),
            trust,
        }
    }

    fn local_agent(&self) -> &ureq::Agent {
        self.local.get_or_init(|| {
            let local = tls::client_config(&self.trust, TrustClass::Local);
            agent(&self.user_agent, None, local)
        })
    }
}

// Redirects stay off so a 3xx returns as a response and the fetch layer
// decides whether the next hop may be contacted. Status codes are never
// errors: a 404 is evidence. The direct agents pass `None` explicitly,
// because ureq's default config reads the proxy environment on its own.
fn agent(
    user_agent: &str,
    proxy: Option<ureq::Proxy>,
    tls_config: std::sync::Arc<rustls::ClientConfig>,
) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .user_agent(user_agent)
        .http_status_as_error(false)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .proxy(proxy)
        .build();
    tls::agent(config, tls_config)
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
        let agent = match (request.trust, &self.proxied, request.via_proxy) {
            (TrustClass::Local, _, _) => self.local_agent(),
            (TrustClass::Public, Some(proxied), true) => proxied,
            (TrustClass::Public, _, _) => &self.direct,
        };
        let trust_set = self.trust.label(request.trust);
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
        }
        .map_err(|e| e.checked_against(trust_set))?;
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
    fn unknown_issuer_is_the_root_rejection_class_and_other_tls_errors_are_not() {
        let unknown = ureq::Error::Rustls(rustls::Error::InvalidCertificate(
            rustls::CertificateError::UnknownIssuer,
        ));
        match TransportError::from(unknown) {
            TransportError::Tls(tls) => assert!(tls.root_rejection),
            other => panic!("{other:?}"),
        }
        let expired = ureq::Error::Rustls(rustls::Error::InvalidCertificate(
            rustls::CertificateError::Expired,
        ));
        match TransportError::from(expired) {
            TransportError::Tls(tls) => {
                assert!(!tls.root_rejection);
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
            err.to_string().contains("chain rejected by the trust set"),
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
