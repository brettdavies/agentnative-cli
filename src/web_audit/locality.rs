//! Host locality classification, a port of the site's `ssrf.ts`.
//!
//! Every host the engine touches, the audit target and each redirect hop
//! alike, is classified here before a connection is made. The site refuses
//! everything non-public; the local engine keeps the same vocabulary and
//! lets the caller decide what each class may do (local targets never reach
//! external DNS resolvers, metadata endpoints are never contacted).
//!
//! Classification order, top to bottom, first rule wins:
//!
//! ```text
//! host (lowercased, trailing dot and brackets stripped, zone id dropped)
//!   |
//!   |-- empty ............................... error: empty host
//!   |-- "localhost" / "*.localhost" ......... Local          (no resolution)
//!   |-- "metadata.google.internal" .......... Metadata       (no resolution)
//!   |-- "*.internal" ........................ Local          (no resolution)
//!   |-- IPv6 literal (contains ':') ......... class of the address
//!   |       unparseable ..................... error
//!   |-- IPv4 literal (any inet_aton form) ... class of the address
//!   |       dotted, short-dotted, decimal, octal, hex
//!   |-- single label (no '.') ............... Local          (no resolution)
//!   '-- everything else ..................... resolve with the system resolver,
//!           then the most restrictive class among the answers:
//!           Metadata > Local > Public; no answers ............ error
//! ```
//!
//! Address classes:
//!
//! - `Metadata`: `169.254.0.0/16` (the IMDS range) and `fd00:ec2::254`.
//! - `Local`: unspecified, loopback, RFC 1918, CGNAT `100.64.0.0/10`, IPv6
//!   unique-local `fc00::/7` and link-local `fe80::/10`; IPv4-mapped and
//!   IPv4-compatible IPv6 forms take the class of the embedded address.
//! - `Public`: anything else.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

/// Where a host lives, decided before any connection is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locality {
    /// Reachable from the open internet: the class anc.dev itself audits.
    Public,
    /// Loopback, private, link-local or unique-local: never leaves the
    /// machine's own network, and never reaches an external DNS resolver.
    Local,
    /// A cloud instance-metadata endpoint: refused outright.
    Metadata,
}

impl Locality {
    /// The class name as it appears in evidence strings.
    pub fn as_str(self) -> &'static str {
        match self {
            Locality::Public => "public",
            Locality::Local => "local",
            Locality::Metadata => "metadata",
        }
    }
}

impl fmt::Display for Locality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a host could not be classified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClassifyError {
    /// The host string is empty.
    EmptyHost,
    /// A bracketed or colon-bearing host that is not an IPv6 address.
    UnparseableIpv6(String),
    /// The system resolver returned no usable address for the host.
    Unresolvable {
        /// The host that failed to resolve.
        host: String,
        /// The resolver's own account of the failure.
        detail: String,
    },
}

impl fmt::Display for ClassifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClassifyError::EmptyHost => f.write_str("empty hostname"),
            ClassifyError::UnparseableIpv6(host) => write!(f, "unparseable ipv6 literal {host}"),
            ClassifyError::Unresolvable { host, detail } => {
                write!(f, "{host} did not resolve: {detail}")
            }
        }
    }
}

impl std::error::Error for ClassifyError {}

/// Name-to-address lookup, injectable so tests can prove a host was
/// classified by shape without any lookup taking place.
pub trait Resolver {
    /// Every address the host resolves to, or the resolver's failure text.
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String>;
}

/// The machine's own configured resolver, through `std::net::ToSocketAddrs`.
/// No third-party resolver is ever consulted for classification.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemResolver;

impl Resolver for SystemResolver {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
        (host, 0)
            .to_socket_addrs()
            .map(|addrs| addrs.map(|a| a.ip()).collect())
            .map_err(|e| e.to_string())
    }
}

/// The IMDS IPv6 address AWS serves instance metadata on.
const IMDS_IPV6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x254);

/// Classify one resolved or literal address.
pub fn classify_ip(ip: IpAddr) -> Locality {
    match ip {
        IpAddr::V4(v4) => classify_ipv4(v4),
        IpAddr::V6(v6) => classify_ipv6(v6),
    }
}

fn classify_ipv4(ip: Ipv4Addr) -> Locality {
    let [a, b, _, _] = ip.octets();
    if a == 169 && b == 254 {
        return Locality::Metadata;
    }
    let local = a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..=127).contains(&b))
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168);
    if local {
        Locality::Local
    } else {
        Locality::Public
    }
}

fn classify_ipv6(ip: Ipv6Addr) -> Locality {
    if ip == IMDS_IPV6 {
        return Locality::Metadata;
    }
    if ip.is_unspecified() || ip.is_loopback() {
        return Locality::Local;
    }
    let seg = ip.segments();
    if seg[0] & 0xfe00 == 0xfc00 || seg[0] & 0xffc0 == 0xfe80 {
        return Locality::Local;
    }
    if let Some(v4) = embedded_ipv4(ip) {
        return classify_ipv4(v4);
    }
    Locality::Public
}

/// The IPv4 address inside an IPv4-mapped (`::ffff:a.b.c.d`) or
/// IPv4-compatible (`::a.b.c.d`) IPv6 address.
fn embedded_ipv4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let seg = ip.segments();
    let prefix_zero = seg[..5].iter().all(|s| *s == 0);
    if prefix_zero && (seg[5] == 0xffff || seg[5] == 0) {
        let v4 = ((seg[6] as u32) << 16) | seg[7] as u32;
        return Some(Ipv4Addr::from(v4));
    }
    None
}

/// Parse an IPv4 literal in any `inet_aton` form: one to four dotted parts,
/// each decimal, octal (leading `0`) or hex (`0x`), with the last part
/// filling the remaining bytes.
pub fn parse_ipv4_literal(host: &str) -> Option<Ipv4Addr> {
    if host.is_empty() {
        return None;
    }
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() > 4 {
        return None;
    }
    let mut values = Vec::with_capacity(parts.len());
    for part in parts {
        values.push(parse_ipv4_part(part)?);
    }
    let last = *values.last()?;
    let last_width_bytes = 4 - (values.len() - 1) as u32;
    if last_width_bytes < 4 && last >= 1u64 << (8 * last_width_bytes) {
        return None;
    }
    if last_width_bytes == 4 && last > u32::MAX as u64 {
        return None;
    }
    if values[..values.len() - 1].iter().any(|v| *v > 255) {
        return None;
    }
    let mut out = last;
    for (i, v) in values[..values.len() - 1].iter().enumerate() {
        out += v << (8 * (3 - i as u32));
    }
    Some(Ipv4Addr::from(out as u32))
}

fn parse_ipv4_part(part: &str) -> Option<u64> {
    if part.is_empty() {
        return None;
    }
    let lower = part.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix("0x") {
        if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        return u64::from_str_radix(hex, 16).ok();
    }
    if lower.starts_with('0') {
        if !lower.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
            return None;
        }
        return u64::from_str_radix(&lower, 8).ok();
    }
    if !lower.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    lower.parse().ok()
}

/// Parse a bracket-stripped IPv6 literal. A zone id (`%eth0`) is dropped,
/// `::` compression is expanded, and a trailing IPv4 tail in any
/// `inet_aton` form fills the last two groups.
pub fn parse_ipv6_literal(host: &str) -> Option<Ipv6Addr> {
    if !host.contains(':') {
        return None;
    }
    let zoneless = host.split('%').next().unwrap_or("");
    let halves: Vec<&str> = zoneless.split("::").collect();
    if halves.len() > 2 {
        return None;
    }
    let head = parse_ipv6_groups(halves[0])?;
    let groups: Vec<u16> = if halves.len() == 2 {
        let tail = parse_ipv6_groups(halves[1])?;
        let fill = 8usize.checked_sub(head.len() + tail.len())?;
        let mut all = head;
        all.extend(std::iter::repeat_n(0, fill));
        all.extend(tail);
        all
    } else {
        head
    };
    if groups.len() != 8 {
        return None;
    }
    let mut seg = [0u16; 8];
    seg.copy_from_slice(&groups);
    Some(Ipv6Addr::from(seg))
}

fn parse_ipv6_groups(segment: &str) -> Option<Vec<u16>> {
    if segment.is_empty() {
        return Some(Vec::new());
    }
    let mut groups = Vec::new();
    for g in segment.split(':') {
        if g.contains('.') {
            let v4 = u32::from(parse_ipv4_literal(g)?);
            groups.push((v4 >> 16) as u16);
            groups.push((v4 & 0xffff) as u16);
        } else {
            if g.is_empty() || g.len() > 4 || !g.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            groups.push(u16::from_str_radix(g, 16).ok()?);
        }
    }
    Some(groups)
}

/// Classify a host by shape alone. `Ok(None)` means the host is a name that
/// only the resolver can place.
pub fn classify_literal(raw_host: &str) -> Result<Option<Locality>, ClassifyError> {
    let host = raw_host.trim().to_ascii_lowercase();
    let host = host.strip_suffix('.').unwrap_or(&host);
    if host.is_empty() {
        return Err(ClassifyError::EmptyHost);
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return Ok(Some(Locality::Local));
    }
    if host == "metadata.google.internal" {
        return Ok(Some(Locality::Metadata));
    }
    if host.ends_with(".internal") {
        return Ok(Some(Locality::Local));
    }
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return parse_ipv6_literal(inner)
            .map(|v6| Some(classify_ipv6(v6)))
            .ok_or_else(|| ClassifyError::UnparseableIpv6(raw_host.to_string()));
    }
    if host.contains(':') {
        return parse_ipv6_literal(host)
            .map(|v6| Some(classify_ipv6(v6)))
            .ok_or_else(|| ClassifyError::UnparseableIpv6(raw_host.to_string()));
    }
    if let Some(v4) = parse_ipv4_literal(host) {
        return Ok(Some(classify_ipv4(v4)));
    }
    if !host.contains('.') {
        return Ok(Some(Locality::Local));
    }
    Ok(None)
}

/// Classify a host: by shape when the form decides it, otherwise by the
/// class of every address the resolver returns, most restrictive first.
pub fn classify_host(host: &str, resolver: &dyn Resolver) -> Result<Locality, ClassifyError> {
    if let Some(class) = classify_literal(host)? {
        return Ok(class);
    }
    let name = host.trim().trim_end_matches('.');
    let addrs = resolver
        .resolve(name)
        .map_err(|detail| ClassifyError::Unresolvable {
            host: name.to_string(),
            detail,
        })?;
    if addrs.is_empty() {
        return Err(ClassifyError::Unresolvable {
            host: name.to_string(),
            detail: "no addresses returned".to_string(),
        });
    }
    let mut class = Locality::Public;
    for ip in addrs {
        match classify_ip(ip) {
            Locality::Metadata => return Ok(Locality::Metadata),
            Locality::Local => class = Locality::Local,
            Locality::Public => {}
        }
    }
    Ok(class)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    /// A resolver that fails the test if the classifier consults it.
    struct PanicResolver;

    impl Resolver for PanicResolver {
        fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
            panic!("classifier resolved {host}; shape-classifiable hosts must not resolve")
        }
    }

    struct FixedResolver(Vec<IpAddr>);

    impl Resolver for FixedResolver {
        fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
            Ok(self.0.clone())
        }
    }

    struct FailingResolver;

    impl Resolver for FailingResolver {
        fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
            Err(format!("no such host: {host}"))
        }
    }

    fn shape(host: &str) -> Locality {
        classify_host(host, &PanicResolver).unwrap_or_else(|e| panic!("{host}: {e}"))
    }

    // One row per form in the site's `web-audit-ssrf.test.ts` table plus the
    // plan's KTD10 additions.
    #[test]
    fn local_forms_classify_without_resolution() {
        let rows = [
            ("loopback ipv6", "[::1]"),
            ("loopback ipv6 bare", "::1"),
            ("loopback ipv4", "127.0.0.1"),
            ("rfc1918 10/8", "10.1.2.3"),
            ("rfc1918 192.168/16", "192.168.0.5"),
            ("rfc1918 172.16/12", "172.20.1.1"),
            ("localhost hostname", "localhost"),
            ("localhost subdomain", "app.localhost"),
            ("localhost trailing dot", "localhost."),
            ("decimal ip literal (127.0.0.1)", "2130706433"),
            ("octal ip literal (127.0.0.1)", "0177.0.0.1"),
            ("hex ip literal (127.0.0.1)", "0x7f.0.0.1"),
            ("short dotted (127.1)", "127.1"),
            ("mixed radix (10.1.2.3)", "0x0a.1.2.3"),
            ("mixed radix octal (192.168.0.1)", "0300.0250.0.1"),
            ("unspecified 0.0.0.0", "0.0.0.0"),
            ("ipv4-mapped ipv6 loopback", "[::ffff:127.0.0.1]"),
            ("ipv4-compatible ipv6 rfc1918", "[::10.0.0.1]"),
            ("cgnat 100.64/10", "100.64.0.1"),
            ("ipv6 unique-local fc00::/7", "[fd00::1]"),
            ("ipv6 link-local fe80::/10", "[fe80::1]"),
            ("ipv6 link-local with zone id", "fe80::1%eth0"),
            ("ipv6 unspecified", "[::]"),
            ("single-label hostname", "intranet"),
            ("dot-internal hostname", "intranet.corp.internal"),
            ("uppercase localhost", "LOCALHOST"),
        ];
        for (label, host) in rows {
            assert_eq!(shape(host), Locality::Local, "{label}: {host}");
        }
    }

    #[test]
    fn metadata_forms_classify_without_resolution() {
        let rows = [
            ("cloud metadata ip", "169.254.169.254"),
            ("link-local ipv4 (metadata range)", "169.254.1.1"),
            ("metadata ip as decimal", "2852039166"),
            ("gcp metadata hostname", "metadata.google.internal"),
            ("ipv4-mapped metadata", "[::ffff:169.254.169.254]"),
            ("aws imds ipv6", "[fd00:ec2::254]"),
        ];
        for (label, host) in rows {
            assert_eq!(shape(host), Locality::Metadata, "{label}: {host}");
        }
    }

    #[test]
    fn public_literals_classify_without_resolution() {
        assert_eq!(shape("93.184.216.34"), Locality::Public);
        assert_eq!(
            shape("[2606:2800:220:1:248:1893:25c8:1946]"),
            Locality::Public
        );
        assert_eq!(
            shape("2606:2800:220:1:248:1893:25c8:1946"),
            Locality::Public
        );
    }

    #[test]
    fn fqdn_classifies_by_resolved_address_class() {
        let public = FixedResolver(vec!["93.184.216.34".parse().unwrap()]);
        assert_eq!(
            classify_host("example.com", &public).unwrap(),
            Locality::Public
        );
        let private = FixedResolver(vec!["10.0.0.7".parse().unwrap()]);
        assert_eq!(
            classify_host("intranet.example.com", &private).unwrap(),
            Locality::Local
        );
        let split = FixedResolver(vec![
            "93.184.216.34".parse().unwrap(),
            "10.0.0.7".parse().unwrap(),
        ]);
        assert_eq!(
            classify_host("split.example.com", &split).unwrap(),
            Locality::Local
        );
        let imds = FixedResolver(vec![
            "10.0.0.7".parse().unwrap(),
            "169.254.169.254".parse().unwrap(),
        ]);
        assert_eq!(
            classify_host("rebinding.example.com", &imds).unwrap(),
            Locality::Metadata
        );
    }

    #[test]
    fn unresolvable_and_empty_hosts_are_errors() {
        let err = classify_host("nope.invalid", &FailingResolver).unwrap_err();
        assert!(matches!(err, ClassifyError::Unresolvable { .. }), "{err}");
        assert!(err.to_string().contains("nope.invalid"));
        let empty = FixedResolver(vec![]);
        assert!(matches!(
            classify_host("empty.example.com", &empty).unwrap_err(),
            ClassifyError::Unresolvable { .. }
        ));
        assert!(matches!(
            classify_host("", &PanicResolver).unwrap_err(),
            ClassifyError::EmptyHost
        ));
        assert!(matches!(
            classify_host("[not-an-address]", &PanicResolver).unwrap_err(),
            ClassifyError::UnparseableIpv6(_)
        ));
    }

    #[test]
    fn ipv4_literal_parser_mirrors_inet_aton() {
        let ip = |s: &str| parse_ipv4_literal(s).map(u32::from);
        assert_eq!(ip("127.0.0.1"), Some(0x7f00_0001));
        assert_eq!(ip("127.1"), Some(0x7f00_0001));
        assert_eq!(ip("2130706433"), Some(0x7f00_0001));
        assert_eq!(ip("0x7f.0.0.1"), Some(0x7f00_0001));
        assert_eq!(ip("0177.0.0.1"), Some(0x7f00_0001));
        assert_eq!(ip("0x7f000001"), Some(0x7f00_0001));
        assert_eq!(ip("1.2.3"), Some(0x0102_0003));
        assert_eq!(ip("256.0.0.1"), None);
        assert_eq!(ip("1.2.3.4.5"), None);
        assert_eq!(ip("1..2"), None);
        assert_eq!(ip("example.com"), None);
        assert_eq!(ip("4294967296"), None);
        assert_eq!(ip(""), None);
    }

    #[test]
    fn ipv6_literal_parser_handles_compression_zone_ids_and_v4_tails() {
        assert_eq!(
            parse_ipv6_literal("::1"),
            Some("::1".parse::<Ipv6Addr>().unwrap())
        );
        assert_eq!(
            parse_ipv6_literal("fe80::1%eth0"),
            Some("fe80::1".parse::<Ipv6Addr>().unwrap())
        );
        assert_eq!(
            parse_ipv6_literal("::ffff:127.0.0.1"),
            Some("::ffff:127.0.0.1".parse::<Ipv6Addr>().unwrap())
        );
        assert_eq!(parse_ipv6_literal("1:2:3:4:5:6:7:8:9"), None);
        assert_eq!(parse_ipv6_literal("1::2::3"), None);
        assert_eq!(parse_ipv6_literal("example.com"), None);
        assert_eq!(parse_ipv6_literal("::fffff"), None);
    }
}
