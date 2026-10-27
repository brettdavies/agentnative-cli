//! The redirect chain: target validation, the hop loop, and its two refusal
//! exits.
//!
//! ```text
//! validate target ---- unparseable / non-http(s) / metadata / unclassifiable
//!       |                                   '--> blocked: ... (no request made)
//!       |-- not https (after the above) ---> not requested: not https
//!       v
//! hop 0 .. hop N   one request, through the audit's request memo when it has one
//!       |
//!       |-- not a redirect, or following is off --> return the response
//!       |
//!       '-- 3xx with Location
//!               |-- unparseable target --------------> blocked: unparseable redirect target
//!               |-- metadata host ------------------->  blocked: ... (redirect hop k)   [exit 1]
//!               |-- class differs from the target's ->  blocked: ... (redirect hop k)   [exit 2]
//!               |-- not https ----------------------> the 3xx itself, refused: not https
//!               |-- hop == N -----------------------> redirect limit exceeded (N hops)
//!               '-- otherwise ----------------------> next hop
//! ```
//!
//! Every hop is classified before it is contacted. A hop that lands in a
//! cloud-metadata range is refused outright, and a hop whose class differs
//! from the resolved target's class is refused too, so a run the user believes
//! is local never follows a redirect off the local network, and a public
//! target never steers the auditor into a private range. The memo answers a
//! hop before this check runs, so a redirect it holds is checked for every
//! caller that would follow it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use url::Url;

use super::hop::{self, HopAnswer, HopContext, HopRecord};
use super::memo::RequestMemo;
use super::proxy;
use super::trust::TrustClass;
use crate::web_audit::locality::{Locality, Resolver, classify_host};
use crate::web_audit::transport::{Request, Transport};

/// The shortest timeout a hop is sent with, however little budget is left.
const MIN_HOP_TIMEOUT: Duration = Duration::from_millis(1);

/// Hops followed before the chain is abandoned.
pub const DEFAULT_MAX_REDIRECTS: usize = 4;

const REDIRECT_STATUSES: [u16; 5] = [301, 302, 303, 307, 308];

/// Whether a status is one the chain follows.
pub fn is_redirect(status: u16) -> bool {
    REDIRECT_STATUSES.contains(&status)
}

/// A URL the chain may contact, with the class it was contacted as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Validated {
    /// The parsed URL.
    pub url: Url,
    /// Its locality class.
    pub class: Locality,
}

/// Validate the audit target before any request is made.
pub fn validate_target(raw: &str, resolver: &dyn Resolver) -> Result<Validated, String> {
    let url = Url::parse(raw).map_err(|_| format!("blocked: unparseable url: {raw}"))?;
    validate_url(url, None, resolver)
}

/// Validate a redirect destination against the target's class.
pub fn validate_hop(
    location: &str,
    current: &Url,
    target_class: Locality,
    hop: usize,
    resolver: &dyn Resolver,
) -> Result<Validated, String> {
    let next = current
        .join(location)
        .map_err(|_| format!("blocked: unparseable redirect target {location}"))?;
    validate_url(next, Some(target_class), resolver)
        .map_err(|reason| format!("{reason} (redirect hop {hop})"))
}

fn validate_url(
    url: Url,
    expected: Option<Locality>,
    resolver: &dyn Resolver,
) -> Result<Validated, String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("blocked: scheme {}: is not http(s)", url.scheme()));
    }
    let host = url
        .host_str()
        .ok_or_else(|| "blocked: empty hostname".to_string())?;
    let class = classify_host(host, resolver).map_err(|e| format!("blocked: {e}"))?;
    // A refusal the site would also make carries the site's wording, so
    // the evidence line matches anc.dev's for the same hop.
    if class == Locality::Metadata {
        let reason = crate::web_audit::locality::site_block_reason(host)
            .unwrap_or_else(|| format!("{host} is a cloud metadata endpoint"));
        return Err(format!("blocked: {reason}"));
    }
    if let Some(expected) = expected
        && class != expected
    {
        let reason = crate::web_audit::locality::site_block_reason(host).unwrap_or_else(|| {
            format!("redirect leaves the {expected} target for a {class} host {host}")
        });
        return Err(format!("blocked: {reason}"));
    }
    Ok(Validated { url, class })
}

/// Where a chain's hops go.
#[derive(Clone, Copy)]
pub enum HopSender<'a> {
    /// Straight to the transport, one request per hop.
    Direct(&'a dyn Transport),
    /// Through the audit's request memo, which answers an identical request
    /// from its record and sends the rest on their own threads.
    Memo(&'a RequestMemo, &'a Arc<dyn Transport + Send + Sync>),
}

impl std::fmt::Debug for HopSender<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HopSender::Direct(_) => f.write_str("Direct"),
            HopSender::Memo(memo, _) => f.debug_tuple("Memo").field(memo).finish(),
        }
    }
}

/// One request shape, re-sent at every hop of a chain.
pub struct ChainRequest<'a> {
    /// Where every hop goes.
    pub sender: HopSender<'a>,
    /// The resolver hop classification consults for names.
    pub resolver: &'a dyn Resolver,
    /// HTTP method.
    pub method: &'a str,
    /// Request headers, sent unchanged at every hop.
    pub headers: &'a [(String, String)],
    /// Request body, sent unchanged at every hop.
    pub body: Option<&'a [u8]>,
    /// When the fetch began; the first hop's budget runs from here.
    pub started: Instant,
    /// The deadline for the whole chain.
    pub deadline: Instant,
    /// The caller's cap for the body of the response it keeps.
    pub max_body_bytes: Option<usize>,
    /// Whether a 3xx is followed or returned as-is.
    pub follow_redirects: bool,
    /// Hops followed before the chain is abandoned.
    pub max_redirects: usize,
}

impl std::fmt::Debug for ChainRequest<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(n, _)| n.as_str()).collect();
        f.debug_struct("ChainRequest")
            .field("method", &self.method)
            .field("headers", &names)
            .field("max_body_bytes", &self.max_body_bytes)
            .field("follow_redirects", &self.follow_redirects)
            .field("max_redirects", &self.max_redirects)
            .finish_non_exhaustive()
    }
}

/// How a chain that produced a response ended.
#[derive(Debug)]
pub enum ChainEnd {
    /// The answer the caller keeps, its body read as far as the caller's
    /// cap; a 3xx included when following is off.
    Answer(HopAnswer),
    /// A redirect to `http` the chain would otherwise follow: the redirect
    /// itself, never followed, and the refusal in the site's wording. The
    /// memo holds the redirect as the target sent it; the refusal is this
    /// caller's reading of it.
    InsecureRedirect {
        /// The 3xx that named the `http` location.
        answer: HopAnswer,
        /// `redirect refused: <status> to <location>: not https`.
        error: String,
    },
}

/// Run the chain from a validated target to the answer the caller keeps.
pub fn follow(chain: &ChainRequest<'_>, target: Validated) -> Result<ChainEnd, String> {
    let via_proxy = proxy::routes_via_proxy(target.class);
    let trust = TrustClass::for_locality(target.class);
    let mut current = target.url;
    for hop in 0..=chain.max_redirects {
        // A first hop's budget runs from the same instant as the deadline,
        // so callers with the same timeout compare equal; a later hop gets
        // what is left of it.
        let started = if hop == 0 {
            chain.started
        } else {
            Instant::now()
        };
        let ctx = HopContext {
            started,
            deadline: chain.deadline,
            body_cap: chain.max_body_bytes,
            keeps_redirect: !chain.follow_redirects,
        };
        let request = Request {
            method: chain.method.to_string(),
            url: current.to_string(),
            headers: chain.headers.to_vec(),
            body: chain.body.map(<[u8]>::to_vec),
            timeout: chain
                .deadline
                .saturating_duration_since(started)
                .max(MIN_HOP_TIMEOUT),
            via_proxy,
            trust,
        };
        let record = match chain.sender {
            HopSender::Direct(transport) => hop::take(transport, &request, &ctx),
            HopSender::Memo(memo, transport) => memo.take(transport, request, ctx),
        };
        let answer = match record {
            HopRecord::Answer(answer) => answer,
            HopRecord::Failure(error) => return Err(error.message),
        };
        if chain.follow_redirects
            && is_redirect(answer.status)
            && let Some(location) = answer.headers.get("location")
        {
            let next = validate_hop(location, &current, target.class, hop + 1, chain.resolver)?;
            if next.url.scheme() != "https" {
                let error = format!(
                    "redirect refused: {} to {location}: not https",
                    answer.status
                );
                return Ok(ChainEnd::InsecureRedirect { answer, error });
            }
            if hop == chain.max_redirects {
                return Err(limit_exceeded(chain.max_redirects));
            }
            current = next.url;
            continue;
        }
        return Ok(ChainEnd::Answer(answer));
    }
    Err(limit_exceeded(chain.max_redirects))
}

fn limit_exceeded(max: usize) -> String {
    format!("redirect limit exceeded ({max} hops)")
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;

    struct PanicResolver;

    impl Resolver for PanicResolver {
        fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
            panic!("{host} must classify without resolution")
        }
    }

    struct PublicResolver;

    impl Resolver for PublicResolver {
        fn resolve(&self, _: &str) -> Result<Vec<IpAddr>, String> {
            Ok(vec!["93.184.216.34".parse().unwrap()])
        }
    }

    struct LocalResolver;

    impl Resolver for LocalResolver {
        fn resolve(&self, _: &str) -> Result<Vec<IpAddr>, String> {
            Ok(vec!["10.0.0.5".parse().unwrap()])
        }
    }

    #[test]
    fn target_validation_names_each_refusal() {
        let err = validate_target("not a url", &PanicResolver).unwrap_err();
        assert_eq!(err, "blocked: unparseable url: not a url");
        let err = validate_target("ftp://example.com/", &PanicResolver).unwrap_err();
        assert_eq!(err, "blocked: scheme ftp: is not http(s)");
        let err = validate_target("http://169.254.169.254/", &PanicResolver).unwrap_err();
        assert_eq!(
            err,
            "blocked: ipv4 169.254.169.254 is in blocked range 169.254.0.0/16"
        );
        let err = validate_target("http://metadata.google.internal/", &PanicResolver).unwrap_err();
        assert_eq!(err, "blocked: internal metadata hostnames are blocked");
        let ok = validate_target("http://localhost:8787/x", &PanicResolver).unwrap();
        assert_eq!(ok.class, Locality::Local);
        let ok = validate_target("https://example.com/", &PublicResolver).unwrap();
        assert_eq!(ok.class, Locality::Public);
    }

    #[test]
    fn hop_validation_refuses_metadata_and_class_crossings_with_the_hop_number() {
        let current = Url::parse("https://example.com/a").unwrap();
        let err = validate_hop(
            "http://[fd00:ec2::254]/",
            &current,
            Locality::Public,
            2,
            &PanicResolver,
        )
        .unwrap_err();
        assert!(err.starts_with("blocked: "), "{err}");
        assert!(err.ends_with("(redirect hop 2)"), "{err}");
        let err = validate_hop(
            "http://10.0.0.5/",
            &current,
            Locality::Public,
            1,
            &PanicResolver,
        )
        .unwrap_err();
        assert_eq!(
            err,
            "blocked: ipv4 10.0.0.5 is in blocked range 10.0.0.0/8 (redirect hop 1)"
        );
        let err = validate_hop(
            "http://intranet.corp/",
            &current,
            Locality::Public,
            1,
            &LocalResolver,
        )
        .unwrap_err();
        assert_eq!(
            err,
            "blocked: redirect leaves the public target for a local host intranet.corp (redirect hop 1)"
        );
        let err = validate_hop(
            "https://example.com/",
            &Url::parse("http://localhost:8787/a").unwrap(),
            Locality::Local,
            1,
            &PublicResolver,
        )
        .unwrap_err();
        assert_eq!(
            err,
            "blocked: redirect leaves the local target for a public host example.com (redirect hop 1)"
        );
        let err =
            validate_hop("http://[bad", &current, Locality::Public, 1, &PanicResolver).unwrap_err();
        assert_eq!(err, "blocked: unparseable redirect target http://[bad");
        let ok = validate_hop("/b", &current, Locality::Public, 1, &PublicResolver).unwrap();
        assert_eq!(ok.url.as_str(), "https://example.com/b");
    }

    #[test]
    fn redirect_statuses_are_the_five_the_site_follows() {
        for status in [301, 302, 303, 307, 308] {
            assert!(is_redirect(status));
        }
        for status in [200, 300, 304, 404] {
            assert!(!is_redirect(status));
        }
    }
}
