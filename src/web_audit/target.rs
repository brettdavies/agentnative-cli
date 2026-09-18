//! Bare-target URL sniffing and normalization.
//!
//! `anc <token>` routes to `web` only when the token is unambiguously a
//! network target under a strict grammar: an explicit scheme, an explicit
//! `:port`, or a dot-bearing token with no path separator whose final
//! label reads as a top-level domain rather than a file extension. A path
//! that exists wins over the grammar, and anything the grammar rejects
//! stays on the offline audit path, so a filename typo never turns into a
//! network attempt.

use url::Url;

use crate::web_audit::locality::{Locality, classify_literal};

/// Final labels that read as file extensions, never as a top-level domain,
/// even where a registry happens to own the same string (`.rs`, `.md`,
/// `.sh`): a dot-bearing token ending in one of these is a filename.
const FILE_EXTENSIONS: &[&str] = &[
    "bak", "bin", "cfg", "conf", "css", "csv", "env", "exe", "gz", "htm", "html", "ini", "jpeg",
    "jpg", "js", "json", "jsx", "lock", "log", "md", "mjs", "pdf", "png", "py", "rb", "rs", "sh",
    "svg", "tar", "tgz", "toml", "ts", "tsx", "txt", "wasm", "xml", "yaml", "yml", "zip",
];

fn has_scheme(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// The host part of a scheme-less token: everything before the first `/`,
/// with a bracketed IPv6 literal kept whole.
fn host_and_rest(token: &str) -> (&str, &str) {
    let end = token.find('/').unwrap_or(token.len());
    (&token[..end], &token[end..])
}

/// `host:port` with a 1-5 digit port, host being a name, an IPv4 literal
/// or a bracketed IPv6 literal.
fn is_port(port: &str) -> bool {
    !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit())
}

fn has_explicit_port(authority: &str) -> bool {
    // A bracketed IPv6 literal carries colons inside the brackets, so the
    // port is whatever follows the closing bracket.
    if let Some(rest) = authority.strip_prefix('[') {
        let Some(close) = rest.find(']') else {
            return false;
        };
        return !rest[..close].is_empty()
            && rest[close + 1..].strip_prefix(':').is_some_and(is_port);
    }
    let Some((host, port)) = authority.rsplit_once(':') else {
        return false;
    };
    // An unbracketed host with a second colon is a bare IPv6 literal, not
    // a host and port.
    !host.is_empty() && !host.contains(':') && is_port(port)
}

/// A dot-bearing token whose final label is alphabetic and not a file
/// extension.
fn is_tld_shaped(authority: &str) -> bool {
    if authority.starts_with('[') || !authority.contains('.') {
        return false;
    }
    let Some(last) = authority.rsplit('.').next() else {
        return false;
    };
    if last.is_empty() || !last.bytes().all(|b| b.is_ascii_alphabetic()) {
        return false;
    }
    let lower = last.to_ascii_lowercase();
    if FILE_EXTENSIONS.contains(&lower.as_str()) {
        return false;
    }
    authority.split('.').all(|label| {
        !label.is_empty()
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// Whether a bare token is a network target under the routing grammar.
pub fn looks_like_url(token: &str) -> bool {
    if token.is_empty() || token.starts_with('-') || token.contains('\\') {
        return false;
    }
    if has_scheme(token) {
        return true;
    }
    if token.starts_with("./") || token.starts_with("../") || token.starts_with('/') {
        return false;
    }
    let (authority, _) = host_and_rest(token);
    if authority.is_empty() {
        return false;
    }
    if has_explicit_port(authority) {
        return true;
    }
    !token.contains('/') && is_tld_shaped(authority)
}

/// A token that routes to `web`: URL-shaped, and not an existing path.
pub fn routes_to_web(token: &str) -> bool {
    looks_like_url(token) && !std::path::Path::new(token).exists()
}

/// The scheme a scheme-less target defaults to: `http` for localhost and
/// local IP literals, `https` for everything else.
fn default_scheme(host: &str) -> &'static str {
    match classify_literal(host) {
        Ok(Some(Locality::Local)) => "http",
        _ => "https",
    }
}

/// Normalize a target into an absolute http(s) URL, or explain why it
/// cannot be one.
pub fn normalize_target(token: &str) -> Result<String, String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("empty target".to_string());
    }
    let candidate = if has_scheme(token) {
        token.to_string()
    } else if token.contains("://") {
        return Err("unsupported scheme: only http and https targets can be audited".to_string());
    } else {
        let (authority, _) = host_and_rest(token);
        let host = if let Some(rest) = authority.strip_prefix('[') {
            rest.split(']')
                .next()
                .map(|h| format!("[{h}]"))
                .unwrap_or_default()
        } else {
            authority
                .rsplit_once(':')
                .map_or(authority, |(h, _)| h)
                .to_string()
        };
        format!("{}://{token}", default_scheme(&host))
    };
    let url = Url::parse(&candidate).map_err(|e| format!("unparseable url {candidate}: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!(
            "unsupported scheme {}: only http and https targets can be audited",
            url.scheme()
        ));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(format!("no host in {candidate}"));
    }
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grammar_routes_schemes_ports_and_tld_shaped_names_only() {
        for token in [
            "https://anc.dev",
            "HTTP://example.com/path",
            "localhost:8787",
            "127.0.0.1:8080",
            "[::1]:8080",
            "staging.internal:8080/docs",
            "anc.dev",
            "docs.example.com",
            "my-site.co.uk",
        ] {
            assert!(looks_like_url(token), "{token}");
        }
        for token in [
            "report.json",
            "main.rs",
            "notes.md",
            "run.sh",
            "docs/site.dev",
            "./localhost:8787",
            "../anc.dev",
            "/tmp/anc.dev",
            "localhost",
            "127.0.0.1",
            "anc.dev/path",
            "-flag",
            "",
            "a..b",
            "example.c0m",
            "host:port",
            "host:123456",
            "C:\\path\\file.dev",
        ] {
            assert!(!looks_like_url(token), "{token}");
        }
    }

    #[test]
    fn an_existing_path_wins_over_the_grammar() {
        let dir = std::env::temp_dir().join(format!("anc-target-{}", std::process::id()));
        let path = dir.join("anc.dev");
        std::fs::create_dir_all(&path).unwrap();
        let token = path.to_string_lossy().into_owned();
        assert!(!routes_to_web(&token));
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let routed = routes_to_web("anc.dev");
        std::env::set_current_dir(cwd).unwrap();
        assert!(!routed);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(routes_to_web("anc.dev"));
    }

    #[test]
    fn scheme_less_targets_default_by_locality() {
        assert_eq!(normalize_target("anc.dev").unwrap(), "https://anc.dev/");
        assert_eq!(
            normalize_target("localhost:8787").unwrap(),
            "http://localhost:8787/"
        );
        assert_eq!(
            normalize_target("127.0.0.1:8080/docs").unwrap(),
            "http://127.0.0.1:8080/docs"
        );
        assert_eq!(
            normalize_target("[::1]:8080").unwrap(),
            "http://[::1]:8080/"
        );
        assert_eq!(
            normalize_target("staging.internal:8080").unwrap(),
            "http://staging.internal:8080/"
        );
        assert_eq!(
            normalize_target("93.184.216.34:8080").unwrap(),
            "https://93.184.216.34:8080/"
        );
        assert_eq!(
            normalize_target("https://Example.com").unwrap(),
            "https://example.com/"
        );
        assert_eq!(
            normalize_target("ftp://x.test").unwrap_err(),
            "unsupported scheme: only http and https targets can be audited"
        );
        assert_eq!(
            normalize_target("wss://x.test/socket").unwrap_err(),
            "unsupported scheme: only http and https targets can be audited"
        );
        assert_eq!(normalize_target("  ").unwrap_err(), "empty target");
        assert!(normalize_target("http://").unwrap_err().contains("http://"));
    }
}
