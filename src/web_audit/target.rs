//! Bare-target URL sniffing and normalization.
//!
//! `anc <token>` routes to `web` only when the token is a network target under a
//! strict grammar: an explicit scheme; an explicit `:port` on `localhost`, an IP
//! literal, or a name under a private-use suffix; or, with or without a port, a
//! dotted name of well-formed labels whose last label is a root-zone top-level
//! domain and does not read as a file extension. A path that exists wins over the
//! grammar, and anything the grammar rejects stays on the offline audit path. A
//! missing filename can still route when its last label is a top-level domain the
//! extension lists do not hold, with or without a `:line`.

use std::ffi::OsString;
use std::net::{Ipv4Addr, Ipv6Addr};

use url::Url;

use super::names;
use crate::argv::format_invocation;

/// Whether `name`'s final dot-separated label reads as a file extension.
fn ends_in_file_extension(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, last)| names::reads_as_file_extension(last))
}

fn has_scheme(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Whether `name` reads as a URL scheme, by RFC 3986's grammar.
fn is_scheme_name(name: &str) -> bool {
    name.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
}

/// Whether a token opens with `scheme://`, whatever the scheme. A `://` later on,
/// in a query or a password, is not a scheme.
#[must_use]
pub fn names_a_scheme(token: &str) -> bool {
    token
        .trim_start()
        .split_once("://")
        .is_some_and(|(name, _)| is_scheme_name(name))
}

/// How much of a token is a scheme name and its colon, when a slash or backslash
/// follows. A one-letter name is a Windows drive.
fn slashed_scheme_len(token: &str) -> Option<usize> {
    token
        .split_once(':')
        .filter(|(name, rest)| {
            name.len() > 1 && is_scheme_name(name) && rest.starts_with(['/', '\\'])
        })
        .map(|(name, _)| name.len() + 1)
}

/// The host part of a scheme-less token: everything before the first `/`, `?`, or
/// `#`, with a bracketed IPv6 literal kept whole.
fn host_and_rest(token: &str) -> (&str, &str) {
    let end = token.find(['/', '?', '#']).unwrap_or(token.len());
    (&token[..end], &token[end..])
}

/// A port a URL can hold.
fn is_port(port: &str) -> bool {
    port.bytes().all(|b| b.is_ascii_digit()) && port.parse::<u16>().is_ok()
}

/// `host:port`, host being a name, an IPv4 literal or a bracketed IPv6 literal.
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

/// The host in an authority that carries an explicit port.
fn host_before_port(authority: &str) -> &str {
    authority
        .rsplit_once(':')
        .map_or(authority, |(host, _)| host)
}

/// Well-formed labels, with a numeric final label only on an IPv4 address, so a
/// version (`python3.12`, `v1.2.3`) does not read as a host.
fn is_host_shaped(host: &str) -> bool {
    let labels_ok = host.split('.').all(|label| {
        !label.is_empty()
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    });
    let numeric_end = host
        .rsplit('.')
        .next()
        .is_some_and(|last| last.bytes().all(|b| b.is_ascii_digit()));
    labels_ok && (!numeric_end || host.parse::<Ipv4Addr>().is_ok())
}

/// A dot-bearing name of well-formed labels whose final label is a root-zone
/// top-level domain and not a listed file extension.
fn is_tld_shaped(authority: &str) -> bool {
    let Some((_, last)) = authority.rsplit_once('.') else {
        return false;
    };
    is_host_shaped(authority) && names::is_root_zone_tld(last) && !ends_in_file_extension(authority)
}

/// An IPv4 address, or a bracketed IPv6 one.
fn is_ip_literal(host: &str) -> bool {
    host.parse::<Ipv4Addr>().is_ok()
        || host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .is_some_and(|address| address.parse::<Ipv6Addr>().is_ok())
}

/// Whether a host with an explicit port reads as a host rather than a `file:line`. An
/// underscore keeps a name from routing, because filenames have them, except under a
/// private-use suffix, where the suffix already says it is a host.
fn port_host_routes(host: &str) -> bool {
    is_ip_literal(host)
        || host.eq_ignore_ascii_case("localhost")
        || (names::is_under_private_use_suffix(host) && is_host_shaped(&host.replace('_', "-")))
        || is_tld_shaped(host)
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
    let (authority, rest) = host_and_rest(token);
    if authority.is_empty() {
        return false;
    }
    if has_explicit_port(authority) {
        return port_host_routes(host_before_port(authority));
    }
    !rest.starts_with('/') && is_tld_shaped(authority)
}

/// The path a bare name would be if it is not a host, or `None` for a token that
/// names a network target outright: a scheme, or a port on `localhost`, an IP literal,
/// or a private-use name. A port on a public-shaped name reads the same as a `file:line`.
#[must_use]
pub fn bare_name_path_form(token: &str) -> Option<String> {
    let (authority, _) = host_and_rest(token);
    let outright = has_scheme(token)
        || (has_explicit_port(authority) && !is_tld_shaped(host_before_port(authority)));
    (!outright).then(|| format!("./{token}"))
}

/// `anc web` with `args`, shell-quoted so the line runs as printed.
pub(in crate::web_audit) fn web_command(args: &[&str]) -> String {
    let argv: Vec<OsString> = ["anc", "web"]
        .iter()
        .chain(args)
        .map(OsString::from)
        .collect();
    format_invocation(&argv)
}

/// Whether a missing path's authority could name a site: `localhost`, a host with a
/// port, or a dotted name, unless its last label is a file extension and no top-level
/// domain. An underscore keeps a name from routing, but compose-style hosts have them
/// too, so it does not rule the name out here.
fn could_name_a_site(authority: &str) -> bool {
    let has_port = has_explicit_port(authority);
    let host = if has_port {
        host_before_port(authority)
    } else {
        authority
    };
    if is_ip_literal(host) {
        return true;
    }
    is_host_shaped(&host.replace('_', "-"))
        && match host.rsplit_once('.') {
            Some((_, last)) => names::is_root_zone_tld(last) || !ends_in_file_extension(host),
            None => has_port || host.eq_ignore_ascii_case("localhost"),
        }
}

/// What to run if a missing path was meant as a website, for a token that could
/// name one. It is a sentence rather than a next step, because a mistyped filename
/// must not become a command an agent runs.
#[must_use]
pub fn website_hint(token: &str) -> Option<String> {
    let (authority, _) = host_and_rest(token);
    let plain = !token.starts_with('-') && !token.contains('\\');
    (plain && could_name_a_site(authority)).then(|| {
        format!(
            "if it names a website, `{}` audits it",
            web_command(&[token])
        )
    })
}

/// Where a target's authority starts, past its scheme and the slashes after it, as
/// the URL parser reads them.
fn authority_start(target: &str) -> usize {
    let scheme = slashed_scheme_len(target).unwrap_or(0);
    target.len() - target[scheme..].trim_start_matches(['/', '\\']).len()
}

/// A target as typed, without the username and password it can carry, for anything
/// a run prints or suggests. It reads the text, because a target that fails to parse
/// is printed too. Such a target has no authority to go by, so everything up to its
/// last `@` comes out, which at worst prints less of it than it holds.
#[must_use]
pub fn redact_userinfo(target: &str) -> String {
    let target = target.trim();
    let (opening, rest) = target.split_at(authority_start(target));
    let parsed = if opening.contains(':') {
        Url::parse(target)
    } else {
        Url::parse(&format!("https://{target}"))
    };
    let end = parsed.map_or(rest.len(), |url| {
        rest.find(|c| matches!(c, '/' | '?' | '#') || (c == '\\' && url.is_special()))
            .unwrap_or(rest.len())
    });
    match rest[..end].rfind('@') {
        Some(at) => format!("{opening}{}", &rest[at + 1..]),
        None => target.to_string(),
    }
}

/// A missing path as its error prints it, without the password of a token that reads
/// as a URL carrying one. `foo@bar/baz` and `@scope/pkg` are paths, and stay as typed.
#[must_use]
pub fn missing_path_shown(token: &str) -> String {
    let first_segment = token[authority_start(token)..]
        .split(['/', '\\'])
        .next()
        .unwrap_or_default();
    let carries_password = first_segment
        .rsplit_once('@')
        .is_some_and(|(user, _)| user.contains(':'));
    if names_a_scheme(token) || slashed_scheme_len(token).is_some() || carries_password {
        redact_userinfo(token)
    } else {
        token.to_string()
    }
}

/// A `name:port` token whose name is an existing path, which makes the port a line
/// number. A file named `localhost` or `127.0.0.1` beside a server is the rare case,
/// so those stay hosts.
fn is_file_line_on_existing_path(token: &str) -> bool {
    let (authority, _) = host_and_rest(token);
    let host = host_before_port(authority);
    has_explicit_port(authority)
        && host.contains('.')
        && !is_ip_literal(host)
        && std::path::Path::new(host).exists()
}

/// A token that routes to `web`: URL-shaped, and not an existing path.
pub fn routes_to_web(token: &str) -> bool {
    looks_like_url(token)
        && !std::path::Path::new(token).exists()
        && !is_file_line_on_existing_path(token)
}

/// Normalize a target into an absolute http(s) URL, or explain why it
/// cannot be one. A scheme-less target defaults to `https`, a local one
/// included: every run sends HTTPS only. An explicit `http` target still
/// normalizes, so the run can end naming the no-plaintext reason rather
/// than calling the target unreadable. The URL carries no userinfo: anc sends none,
/// so everything downstream prints a URL with no password to leak.
pub fn normalize_target(token: &str) -> Result<String, String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("empty target".to_string());
    }
    let candidate = if has_scheme(token) {
        token.to_string()
    } else if names_a_scheme(token) {
        return Err("unsupported scheme: only http and https targets can be audited".to_string());
    } else if slashed_scheme_len(token).is_some() {
        return Err(format!(
            "unparseable url {}: a scheme takes two slashes",
            redact_userinfo(token)
        ));
    } else {
        format!("https://{token}")
    };
    let shown = redact_userinfo(&candidate);
    let mut url = Url::parse(&candidate).map_err(|e| format!("unparseable url {shown}: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!(
            "unsupported scheme {}: only http and https targets can be audited",
            url.scheme()
        ));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(format!("no host in {shown}"));
    }
    // Neither fails on an http or https URL with a host, which is all that gets here.
    let _ = url.set_username("");
    let _ = url.set_password(None);
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
            "LOCALHOST:8787",
            "127.0.0.1:8080",
            "1.2.3.4:80",
            "[::1]:8080",
            "staging.internal:8080/docs",
            "my_host.internal:8080",
            "printer.local:631",
            "nas.home.arpa:8443",
            "box.lan:80",
            "api.example.com:8443",
            "[::1]:80/docs",
            "api.example.com:8443/x",
            "127.0.0.1:9?x=1",
            "anc.dev?x=1",
            "anc.dev#top",
            // A missing file on a domain the extension lists do not hold still routes.
            "Foo.xyz:12",
            "notes.xyz",
            "anc.dev",
            "ANC.DEV",
            "docs.example.com",
            "my-site.co.uk",
            "example.fr",
            "example.xn--p1ai",
        ] {
            assert!(looks_like_url(token), "{token}");
        }
        for token in [
            "report.json",
            "main.rs",
            "notes.md",
            "run.sh",
            // Not top-level domains, and on no extension list: the root zone
            // is what keeps these offline.
            "missing.go",
            "a.out",
            "main.c",
            "repo.git",
            "conf.d",
            "secrets.pem",
            "photo.heic",
            "staging.internal",
            "printer.local",
            // Top-level domains that read as file extensions.
            "main.tf",
            "setup.cc",
            "libfoo.so",
            "script.pl",
            "Module.pm",
            "figure.ps",
            "rules.mk",
            "App.java",
            "archive.zip",
            "clip.mov",
            // A `file:line` reference takes the explicit-port branch.
            "main.rs:42",
            "README.md:12",
            "foo.py:7",
            "main.go:42",
            "Main.GO:42",
            "Program.cs:42",
            "app.vue:3",
            "conf.d:3",
            "stub.pyi:3",
            "page.mdx:3",
            "view.erb:3",
            "tasks.rake:3",
            "lab.ipynb:3",
            "flake.nix:12",
            "main.hcl:7",
            "schema.graphql:3",
            "fix.patch:9",
            "build.gradle.kts:4",
            "go.mod:3",
            // A port does not make a host of a name on no known domain.
            "staging.corp:8080",
            "my_host.corp:8080",
            "[main.rs]:3",
            // The URL parser refuses a zone, so routing it would only dead-end.
            "[fe80::1%25en0]:80",
            // No URL holds these ports, so the number reads as a line.
            "127.0.0.1:99999",
            "Foo.xyz:70000",
            // Top-level domains that read as file extensions, in the bare form.
            "requirements.in",
            "configure.ac",
            "Makefile.am",
            "notes.app",
            // A private-use suffix takes a port.
            ".env.local",
            "foo.internal",
            // Not a host at all, so the port does not make it one.
            ".bashrc:12",
            "a..b:80",
            "my file.xyz:12",
            "python3.12:3",
            "v1.2.3:8080",
            "1.2.3.999:80",
            // A dotless host routes with a port only as `localhost`.
            "Makefile:12",
            "README:12",
            "TODO:3",
            "node:20",
            "myserver:8080",
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
    fn a_printed_target_carries_no_password() {
        assert_eq!(
            redact_userinfo("https://user:secret@anc.dev/x"),
            "https://anc.dev/x"
        );
        assert_eq!(redact_userinfo("https://anc.dev/x"), "https://anc.dev/x");
        assert_eq!(redact_userinfo("anc.dev"), "anc.dev");
        assert_eq!(redact_userinfo("localhost:8787"), "localhost:8787");
        // Without a scheme, and where the URL parser never gets a say.
        assert_eq!(redact_userinfo("user:pw@127.0.0.1:9"), "127.0.0.1:9");
        assert_eq!(redact_userinfo("user@anc.dev/x"), "anc.dev/x");
        assert_eq!(redact_userinfo("ftp://user:pw@h/"), "ftp://h/");
        assert_eq!(
            redact_userinfo("https://user:pw@bad host"),
            "https://bad host"
        );
        assert_eq!(redact_userinfo("https://u:p@ss@anc.dev"), "https://anc.dev");
        // An `@` past the authority is not userinfo.
        assert_eq!(
            redact_userinfo("https://anc.dev/a@b?c=d@e"),
            "https://anc.dev/a@b?c=d@e"
        );
        assert_eq!(redact_userinfo("anc.dev?x=a@b"), "anc.dev?x=a@b");
        // A `://` past the authority is no scheme, and the parser's leading slashes
        // are no authority.
        assert_eq!(redact_userinfo("user:pw@h/?u=http://x"), "h/?u=http://x");
        assert_eq!(redact_userinfo("user:a://b@h"), "h");
        assert_eq!(redact_userinfo("//user:pw@h"), "//h");
        assert_eq!(redact_userinfo("https:///user:pw@h"), "https:///h");
        assert_eq!(redact_userinfo("https:/user:pw@h"), "https:/h");
        assert_eq!(redact_userinfo("https:\\\\user:pw@h"), "https:\\\\h");
        // Where a password holds a delimiter, the URL does not parse, so the last `@`
        // decides.
        assert_eq!(redact_userinfo("https://user:pa/ss@h/"), "https://h/");
        assert_eq!(redact_userinfo("https://user:pa#ss@h/"), "https://h/");
        assert_eq!(redact_userinfo("user:pa?ss@h"), "h");
        assert_eq!(redact_userinfo("https://bad host/a@b"), "https://b");
        // A backslash ends the authority only where the URL parser says it does.
        assert_eq!(redact_userinfo("ssh://u:p\\q@h"), "ssh://h");
        assert_eq!(redact_userinfo("ftp:\\\\u:pw@h"), "ftp:\\\\h");
        assert_eq!(redact_userinfo("ftp:/u:pw@h"), "ftp:/h");
    }

    #[test]
    fn a_missing_path_keeps_its_name_unless_it_carries_a_password() {
        assert_eq!(missing_path_shown("user:pw@h:8443"), "h:8443");
        assert_eq!(missing_path_shown("user:pw@h/x"), "h/x");
        assert_eq!(missing_path_shown("https://user:pw@h"), "https://h");
        assert_eq!(missing_path_shown("https:/user:pw@h"), "https:/h");
        assert_eq!(missing_path_shown("//user:pw@h"), "//h");
        assert_eq!(missing_path_shown("me@corp.com:pw@h:8443"), "h:8443");
        for path in [
            "foo@bar/baz",
            "@scope/pkg",
            "src/a:b@c",
            "report.json",
            "a:b",
            "git@github.com:org/repo",
            "C:/Users/me@corp/x",
        ] {
            assert_eq!(missing_path_shown(path), path);
        }
    }

    #[test]
    fn a_reason_that_quotes_the_target_carries_no_password() {
        for target in [
            "https://user:secret@bad host",
            "https://user:secret@",
            "https:/user:secret@h",
            "https://user:se/secret@h",
            "ftp:\\\\u:secret@h",
            "ftp:/u:secret@h",
        ] {
            let reason = normalize_target(target).unwrap_err();
            assert!(!reason.contains("secret"), "{reason}");
        }
    }

    #[test]
    fn a_port_on_a_public_shaped_name_can_be_a_file_line() {
        for token in ["Foo.xyz:12", "api.example.com:8443", "anc.dev"] {
            assert_eq!(bare_name_path_form(token), Some(format!("./{token}")));
        }
        for token in [
            "localhost:8787",
            "127.0.0.1:9",
            "[::1]:80",
            "staging.internal:8080",
            "https://anc.dev",
        ] {
            assert_eq!(bare_name_path_form(token), None, "{token}");
        }
    }

    #[test]
    fn a_normalized_target_carries_no_userinfo() {
        for target in ["https://user:secret@anc.dev/x", "user:secret@anc.dev/x"] {
            assert_eq!(normalize_target(target).unwrap(), "https://anc.dev/x");
        }
    }

    #[test]
    fn only_a_leading_scheme_is_a_scheme() {
        assert_eq!(
            normalize_target("localhost:9/?next=https://x.dev").unwrap(),
            "https://localhost:9/?next=https://x.dev"
        );
        assert!(!names_a_scheme("localhost:9/?next=https://x.dev"));
        assert!(!names_a_scheme("user:a://b@h"));
        assert!(names_a_scheme("ftp://x"));
        assert!(names_a_scheme(" https://x"));
        assert!(
            normalize_target("https:/h")
                .unwrap_err()
                .contains("a scheme takes two slashes")
        );
        assert!(normalize_target("http:\\\\h").is_err());
        assert!(normalize_target("ftp:/h").is_err());
        // A one-letter name is a Windows drive, not a mistyped scheme.
        assert!(normalize_target("C:\\x").is_ok());
    }

    #[test]
    fn a_missing_path_that_could_be_a_host_points_at_the_verb() {
        for token in [
            "wp.pl",
            "printer.local",
            "host:8080",
            "example.com/",
            "docs.example.com/guide",
            "my_service:8080",
            "staging.corp:8080",
            "localhost",
            "127.0.0.1",
            "1.2.3.4:80",
        ] {
            let hint = website_hint(token).unwrap_or_default();
            assert!(
                hint.contains(&format!("anc web {token}")),
                "{token}: {hint}"
            );
        }
        for (token, quoted) in [
            ("[::1]:80", "'[::1]:80'"),
            ("[::1]", "'[::1]'"),
            ("127.0.0.1:9?x=1", "'127.0.0.1:9?x=1'"),
        ] {
            assert!(
                website_hint(token)
                    .unwrap_or_default()
                    .contains(&format!("anc web {quoted}")),
                "{token}"
            );
        }
        for token in [
            "./wp.pl",
            "src/main.go",
            "Makefile",
            ".env",
            "-x.y",
            "missing.txt",
            "main.go",
            "main.go:42",
            "my file.txt",
            "v1.2.3",
        ] {
            assert_eq!(website_hint(token), None, "{token}");
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
        // A `file:line` on the same path is the same file, and a server's port stays one.
        let file_line = routes_to_web("anc.dev:12");
        let file_line_with_path = routes_to_web("anc.dev:12/x");
        std::fs::create_dir(dir.join("localhost")).unwrap();
        let local = routes_to_web("localhost:8787");
        std::env::set_current_dir(cwd).unwrap();
        assert!(!routed);
        assert!(!file_line);
        assert!(!file_line_with_path);
        assert!(local);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(routes_to_web("anc.dev"));
        assert!(routes_to_web("anc.dev:12"));
    }

    #[test]
    fn scheme_less_targets_default_to_https_local_ones_included() {
        assert_eq!(normalize_target("anc.dev").unwrap(), "https://anc.dev/");
        assert_eq!(
            normalize_target("localhost:8787").unwrap(),
            "https://localhost:8787/"
        );
        assert_eq!(
            normalize_target("127.0.0.1:8080/docs").unwrap(),
            "https://127.0.0.1:8080/docs"
        );
        assert_eq!(
            normalize_target("[::1]:8080").unwrap(),
            "https://[::1]:8080/"
        );
        assert_eq!(
            normalize_target("staging.internal:8080").unwrap(),
            "https://staging.internal:8080/"
        );
        assert_eq!(
            normalize_target("http://localhost:8787").unwrap(),
            "http://localhost:8787/"
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
