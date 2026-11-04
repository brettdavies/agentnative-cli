//! The one URL host a run's bearer token belongs to, and the rule that keeps
//! the token off every other. A token goes out only when its own channel names
//! that host and the endpoint of record is on it: `ANC_WEB_TOKEN_HOST` binds
//! `ANC_WEB_TOKEN`, and `--token-host` binds `--token-stdin`. An audited site can
//! suggest a `--token-host` value, so that flag alone never releases a token
//! that was already in the environment.

use std::ffi::OsStr;
use std::fmt;

use url::Url;

use super::credential::TokenSource;
use super::locality::normalized_host;

/// What a URL host with no port means, since every run sends HTTPS only.
const HTTPS_PORT: u16 = 443;

/// A URL host: a hostname or IP literal and its port, spelled as [`Url`]
/// normalizes it with a trailing dot dropped, so two spellings of one host
/// compare equal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenHost {
    host: String,
    port: u16,
}

impl TokenHost {
    /// `raw` as a URL host: a hostname or IP literal with an optional port.
    /// `None` when it carries a scheme, userinfo, path, query or fragment, or
    /// names no host.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        // `Url` drops tabs and newlines and decodes percent escapes, so what
        // it parses is not always what was typed.
        let delimited = raw.contains(|c: char| c.is_whitespace() || "/\\?#@%".contains(c));
        if delimited || raw.ends_with(':') {
            return None;
        }
        let parsed = Self::of_url(&format!("https://{raw}/"))?;
        parsed.is_name_or_literal().then_some(parsed)
    }

    /// The URL host of `url`, or `None` when it has none.
    #[must_use]
    pub fn of_url(url: &str) -> Option<Self> {
        let url = Url::parse(url).ok()?;
        Some(TokenHost {
            host: normalized_host(url.host_str()?),
            port: url.port_or_known_default()?,
        })
    }

    // `Url` accepts `*` and an empty label in a domain.
    fn is_name_or_literal(&self) -> bool {
        self.host.starts_with('[')
            || self.host.split('.').all(|label| {
                !label.is_empty()
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            })
    }
}

impl fmt::Display for TokenHost {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.port == HTTPS_PORT {
            f.write_str(&self.host)
        } else {
            write!(f, "{}:{}", self.host, self.port)
        }
    }
}

/// Where a token host was named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenHostSource {
    /// The `--token-host` flag.
    Flag,
    /// The `ANC_WEB_TOKEN_HOST` environment variable.
    Env,
}

/// A named token host that is not a URL host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidTokenHost {
    /// Where it was named.
    pub source: TokenHostSource,
    /// The value as given.
    pub value: String,
}

/// Why a token's own channel binds it to no host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnboundToken {
    /// `ANC_WEB_TOKEN` with `ANC_WEB_TOKEN_HOST` unset, and whether
    /// `--token-host` was given, which does not bind it.
    EnvToken {
        /// `--token-host` was given.
        flag_given: bool,
    },
    /// `ANC_WEB_TOKEN` with `ANC_WEB_TOKEN_HOST` and `--token-host` naming
    /// different hosts.
    HostsDisagree {
        /// What `ANC_WEB_TOKEN_HOST` names.
        variable: String,
        /// What `--token-host` names.
        flag: String,
    },
    /// A token on stdin with no `--token-host`, and whether
    /// `ANC_WEB_TOKEN_HOST` was set, which does not bind it.
    StdinToken {
        /// `ANC_WEB_TOKEN_HOST` was set.
        variable_set: bool,
    },
}

/// What binds a run's token to a URL host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenBinding {
    /// The token's own channel names this host.
    Bound(TokenHost),
    /// It names none, for this reason.
    Unbound(UnboundToken),
}

/// What binds the token read from `source`, or `None` with no token. `flag` is
/// `--token-host` and is checked on any run. `variable` is the value of
/// `ANC_WEB_TOKEN_HOST`, read only for `ANC_WEB_TOKEN`, and set to nothing it
/// names no host.
pub fn read_token_binding(
    source: Option<TokenSource>,
    flag: Option<&str>,
    variable: Option<&OsStr>,
) -> Result<Option<TokenBinding>, InvalidTokenHost> {
    let flag = flag.map(parse_flag).transpose()?;
    let variable = variable.filter(|value| !value.is_empty());
    let binding = match source {
        None => return Ok(None),
        Some(TokenSource::Stdin) => match flag {
            Some(flag) => TokenBinding::Bound(flag),
            None => TokenBinding::Unbound(UnboundToken::StdinToken {
                variable_set: variable.is_some(),
            }),
        },
        Some(TokenSource::Env) => match (variable.map(parse_variable).transpose()?, flag) {
            (None, flag) => TokenBinding::Unbound(UnboundToken::EnvToken {
                flag_given: flag.is_some(),
            }),
            (Some(variable), Some(flag)) if variable != flag => {
                TokenBinding::Unbound(UnboundToken::HostsDisagree {
                    variable: variable.to_string(),
                    flag: flag.to_string(),
                })
            }
            (Some(variable), _) => TokenBinding::Bound(variable),
        },
    };
    Ok(Some(binding))
}

fn parse_flag(raw: &str) -> Result<TokenHost, InvalidTokenHost> {
    TokenHost::parse(raw).ok_or_else(|| InvalidTokenHost {
        source: TokenHostSource::Flag,
        value: raw.to_string(),
    })
}

fn parse_variable(value: &OsStr) -> Result<TokenHost, InvalidTokenHost> {
    value
        .to_str()
        .and_then(TokenHost::parse)
        .ok_or_else(|| InvalidTokenHost {
            source: TokenHostSource::Env,
            value: value.to_string_lossy().into_owned(),
        })
}

/// Why a run kept the token it held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenNotSent {
    /// The URL host the token is bound to, when something names one.
    pub bound_host: Option<String>,
    /// The endpoint of record, when the run has one.
    pub endpoint: Option<String>,
    /// That endpoint came from a declared host.
    pub followed: bool,
}

/// Why a token bound to `named` cannot go to the endpoint of record, or `None`
/// when that endpoint is on exactly the named host.
#[must_use]
pub fn token_not_sent(
    named: Option<&TokenHost>,
    endpoint: Option<&str>,
    followed: bool,
) -> Option<TokenNotSent> {
    let reached = endpoint.and_then(TokenHost::of_url);
    if named.is_some() && named == reached.as_ref() {
        return None;
    }
    Some(TokenNotSent {
        bound_host: named.map(ToString::to_string),
        endpoint: endpoint.map(str::to_string),
        followed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(raw: &str) -> TokenHost {
        TokenHost::parse(raw).unwrap_or_else(|| panic!("{raw} is a host"))
    }

    #[test]
    fn a_url_host_takes_a_name_or_literal_and_an_optional_port() {
        for raw in [
            "mcp.example.com",
            "localhost",
            "localhost:8787",
            "127.0.0.1",
            "127.0.0.1:8787",
            "[::1]",
            "[::1]:8787",
            "internal_host.test",
            "b\u{fc}cher.example",
        ] {
            assert!(TokenHost::parse(raw).is_some(), "{raw}");
        }
    }

    #[test]
    fn anything_else_in_a_url_is_refused() {
        for raw in [
            "",
            ".",
            "https://mcp.example.com",
            "https:mcp.example.com",
            "mcp.example.com/",
            "mcp.example.com/mcp",
            "mcp.example.com\\mcp",
            "mcp.example.com?x=1",
            "mcp.example.com#x",
            "user@mcp.example.com",
            "user:pw@mcp.example.com",
            "mcp.example.com:",
            "mcp.example.com:notaport",
            "mcp.example.com:65536",
            "*.example.com",
            "mcp..example.com",
            "mcp%2eexample.com",
            "two words",
            " mcp.example.com",
            "mcp\t.example.com",
            "[not-an-address]",
            "::1",
        ] {
            assert_eq!(TokenHost::parse(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn spelling_is_compared_as_the_locality_code_compares_it() {
        let bound = host("mcp.example.com");
        for same in [
            "MCP.Example.COM",
            "mcp.example.com.",
            "MCP.EXAMPLE.com.",
            "mcp.example.com:443",
            "mcp.example.com.:443",
        ] {
            assert_eq!(host(same), bound, "{same}");
        }
        assert_eq!(host("[0:0:0:0:0:0:0:1]"), host("[::1]"));
        assert_eq!(host("b\u{fc}cher.example"), host("xn--bcher-kva.example"));
        assert_ne!(host("mcp.example.com:8443"), bound);
        assert_ne!(host("example.com"), bound);
    }

    #[test]
    fn a_url_host_prints_its_port_only_when_it_is_not_443() {
        assert_eq!(host("MCP.example.com.:443").to_string(), "mcp.example.com");
        assert_eq!(host("localhost:8787").to_string(), "localhost:8787");
        assert_eq!(host("[::1]:8787").to_string(), "[::1]:8787");
    }

    #[test]
    fn an_endpoint_is_on_a_host_by_its_host_and_port() {
        let on = |raw: &str, url: &str| TokenHost::of_url(url) == Some(host(raw));
        assert!(on("mcp.example.com", "https://MCP.example.com./mcp"));
        assert!(on("mcp.example.com", "https://mcp.example.com:443/mcp"));
        assert!(on("mcp.example.com:8443", "https://mcp.example.com:8443/"));
        assert!(!on("mcp.example.com", "https://mcp.example.com:8443/mcp"));
        assert!(!on("mcp.example.com:8443", "https://mcp.example.com/mcp"));
        assert!(!on("example.com", "https://mcp.example.com/mcp"));
        // An `http` URL with no port is on port 80, not on the host a bare name means.
        assert!(!on("mcp.example.com", "http://mcp.example.com/mcp"));
    }

    #[test]
    fn only_an_endpoint_on_the_named_host_gets_the_token() {
        let bound = host("mcp.example.com");
        let endpoint = "https://mcp.example.com/mcp";
        assert_eq!(token_not_sent(Some(&bound), Some(endpoint), true), None);
        assert_eq!(token_not_sent(Some(&bound), Some(endpoint), false), None);
        assert_eq!(
            token_not_sent(None, Some(endpoint), false),
            Some(TokenNotSent {
                bound_host: None,
                endpoint: Some(endpoint.to_string()),
                followed: false,
            })
        );
        assert_eq!(
            token_not_sent(Some(&bound), None, false),
            Some(TokenNotSent {
                bound_host: Some("mcp.example.com".to_string()),
                endpoint: None,
                followed: false,
            })
        );
        assert_eq!(
            token_not_sent(Some(&bound), Some("https://evil.example/mcp"), true),
            Some(TokenNotSent {
                bound_host: Some("mcp.example.com".to_string()),
                endpoint: Some("https://evil.example/mcp".to_string()),
                followed: true,
            })
        );
        assert!(
            token_not_sent(None, None, false).is_some(),
            "nothing named and nothing found still reads as kept"
        );
    }

    fn bound(raw: &str) -> Result<Option<TokenBinding>, InvalidTokenHost> {
        Ok(Some(TokenBinding::Bound(host(raw))))
    }

    fn unbound(why: UnboundToken) -> Result<Option<TokenBinding>, InvalidTokenHost> {
        Ok(Some(TokenBinding::Unbound(why)))
    }

    fn read(
        source: TokenSource,
        flag: Option<&str>,
        variable: Option<&str>,
    ) -> Result<Option<TokenBinding>, InvalidTokenHost> {
        read_token_binding(Some(source), flag, variable.map(OsStr::new))
    }

    #[test]
    fn the_variable_binds_a_token_from_the_environment_and_the_flag_alone_does_not() {
        let env = TokenSource::Env;
        assert_eq!(read(env, None, Some("x.dev")), bound("x.dev"));
        assert_eq!(read(env, Some("X.dev."), Some("x.dev")), bound("x.dev"));
        assert_eq!(
            read(env, Some("x.dev"), None),
            unbound(UnboundToken::EnvToken { flag_given: true })
        );
        assert_eq!(
            read(env, None, None),
            unbound(UnboundToken::EnvToken { flag_given: false })
        );
        assert_eq!(
            read(env, Some("x.dev"), Some("")),
            unbound(UnboundToken::EnvToken { flag_given: true }),
            "a variable set to nothing names no host"
        );
        assert_eq!(
            read(env, Some("y.dev"), Some("x.dev")),
            unbound(UnboundToken::HostsDisagree {
                variable: "x.dev".to_string(),
                flag: "y.dev".to_string(),
            })
        );
        assert_eq!(
            read(env, None, Some("https://x.dev")),
            Err(InvalidTokenHost {
                source: TokenHostSource::Env,
                value: "https://x.dev".to_string(),
            })
        );
    }

    #[test]
    fn the_flag_binds_a_token_on_stdin_and_the_variable_does_not() {
        let stdin = TokenSource::Stdin;
        assert_eq!(read(stdin, Some("x.dev"), None), bound("x.dev"));
        assert_eq!(read(stdin, Some("x.dev"), Some("y.dev")), bound("x.dev"));
        assert_eq!(
            read(stdin, Some("x.dev"), Some("https://y.dev")),
            bound("x.dev"),
            "the variable is not read for a token on stdin"
        );
        assert_eq!(
            read(stdin, None, Some("x.dev")),
            unbound(UnboundToken::StdinToken { variable_set: true })
        );
        assert_eq!(
            read(stdin, None, None),
            unbound(UnboundToken::StdinToken {
                variable_set: false
            })
        );
    }

    #[test]
    fn a_typed_flag_is_checked_on_any_run_and_the_variable_only_with_a_token() {
        let invalid_flag = Err(InvalidTokenHost {
            source: TokenHostSource::Flag,
            value: String::new(),
        });
        assert_eq!(read_token_binding(None, Some(""), None), invalid_flag);
        assert_eq!(read(TokenSource::Env, Some(""), None), invalid_flag);
        assert_eq!(read(TokenSource::Stdin, Some(""), None), invalid_flag);
        assert_eq!(
            read_token_binding(None, None, Some(OsStr::new("https://x.dev"))),
            Ok(None)
        );
        assert_eq!(read_token_binding(None, Some("x.dev"), None), Ok(None));
    }

    #[cfg(unix)]
    #[test]
    fn a_variable_that_is_not_text_is_invalid() {
        use std::os::unix::ffi::OsStrExt as _;
        let read = read_token_binding(
            Some(TokenSource::Env),
            None,
            Some(OsStr::from_bytes(&[0x78, 0xff])),
        );
        assert_eq!(
            read.map_err(|e| e.source),
            Err(TokenHostSource::Env),
            "bytes that are not UTF-8 name no host"
        );
    }
}
