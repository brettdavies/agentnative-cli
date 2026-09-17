//! Per-request proxy selection.
//!
//! A configured proxy applies to public paths only: the audit target when it
//! classifies public, the DNS-over-HTTPS resolvers, and any other host off
//! the local network. A local or private target connects directly, so an
//! internal hostname never leaves the machine's own network through a proxy
//! the environment happens to name. `NO_PROXY` is honored by the proxy
//! itself on the public path.

use crate::web_audit::locality::Locality;

/// The proxy named by `ALL_PROXY`, `HTTPS_PROXY` or `HTTP_PROXY`, with
/// `NO_PROXY` attached, or none when the environment names none.
pub fn proxy_from_env() -> Option<ureq::Proxy> {
    ureq::Proxy::try_from_env()
}

/// Whether requests to a host of this class go through a configured proxy.
pub fn routes_via_proxy(class: Locality) -> bool {
    class == Locality::Public
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_hosts_route_through_a_proxy() {
        assert!(routes_via_proxy(Locality::Public));
        assert!(!routes_via_proxy(Locality::Local));
        assert!(!routes_via_proxy(Locality::Metadata));
    }
}
