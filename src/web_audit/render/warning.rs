//! A web run's stderr warnings, each `warning: <code>: <message>`, so a
//! consumer can branch on the code and a person can read the message.

use super::failure::audit_path_command;
use super::invocation::Invocation;
use crate::spawn_env::{WEB_TOKEN_ENV, WEB_TOKEN_HOST_ENV};
use crate::web_audit::target::redact_userinfo;
use crate::web_audit::token_host::{TokenNotSent, UnboundToken};

/// The closed set of codes a web run's stderr warning carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarningCode {
    /// The run hit a check this build cannot score, so its score does not
    /// compare with anc.dev's.
    NonComparable,
    /// A bare `anc <target>` ran as `anc web <target>`.
    RoutedToWeb,
    /// The run held a token and sent it nowhere.
    TokenNotSent,
}

impl WarningCode {
    /// The code as it prints.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NonComparable => "non-comparable",
            Self::RoutedToWeb => "routed-to-web",
            Self::TokenNotSent => "token-not-sent",
        }
    }
}

/// One warning line, without its newline.
#[must_use]
pub fn warning_line(code: WarningCode, message: &str) -> String {
    format!("warning: {}: {message}", code.as_str())
}

/// Says what a sniffed target ran as, and names the path form where a typo of
/// a path is what the token could be.
#[must_use]
pub fn routed_to_web_message(invocation: &Invocation<'_>) -> String {
    let token = redact_userinfo(invocation.target);
    let ran_as = format!(
        "`{}` ran as `{}`",
        invocation.as_typed(&token),
        invocation.naming_web(&token)
    );
    match audit_path_command(&token) {
        Some(audit) => format!("{ran_as}; `{audit}` audits a path by that name"),
        None => ran_as,
    }
}

/// Says the token in the environment stayed there, and which command sends it:
/// the one the caller ran, flags and all, with `web` named.
#[must_use]
pub fn token_not_read_message(env_name: &str, invocation: &Invocation<'_>) -> String {
    format!(
        "{env_name} is set and not read: only `{}` with {WEB_TOKEN_HOST_ENV} set sends it",
        invocation.naming_web(&redact_userinfo(invocation.target))
    )
}

/// Says why the token stayed on this machine. No sentence offers the endpoint's
/// host as the value to name: an audited site chooses that host, and the host a
/// token belongs to is the operator's to know.
#[must_use]
pub fn token_not_sent_message(not_sent: &TokenNotSent, unbound: Option<&UnboundToken>) -> String {
    if let Some(unbound) = unbound {
        return unbound_message(unbound);
    }
    let Some(bound) = &not_sent.bound_host else {
        return "the token was not sent because no host is named for it".to_string();
    };
    match not_sent.endpoint.as_deref().map(redact_userinfo) {
        None => format!(
            "the token was not sent: it is bound to {bound}, and this run has no MCP endpoint there"
        ),
        Some(endpoint) if not_sent.followed => format!(
            "the token was not sent: it is bound to {bound}, and the MCP endpoint is {endpoint}, \
             on another host"
        ),
        Some(endpoint) => format!(
            "the token was not sent: it is bound to {bound}, and the audited site's own MCP \
             endpoint {endpoint} outranks any endpoint the site declares on another host"
        ),
    }
}

fn unbound_message(unbound: &UnboundToken) -> String {
    match unbound {
        UnboundToken::EnvToken { flag_given: false } => format!(
            "{WEB_TOKEN_ENV} was not sent because {WEB_TOKEN_HOST_ENV} does not name the host it \
             was issued for; set that variable, or pipe the token to `--token-stdin --token-host \
             <host>`"
        ),
        UnboundToken::EnvToken { flag_given: true } => format!(
            "{WEB_TOKEN_ENV} was not sent because --token-host alone does not bind it, and an \
             audited site can suggest that flag; set {WEB_TOKEN_HOST_ENV} to the host the token \
             was issued for"
        ),
        UnboundToken::HostsDisagree { variable, flag } => format!(
            "{WEB_TOKEN_ENV} was not sent because {WEB_TOKEN_HOST_ENV} names {variable} and \
             --token-host names {flag}; the two must name the same host"
        ),
        UnboundToken::StdinToken {
            variable_set: false,
        } => "the token on stdin was not sent because --token-host does not name the host it was \
              issued for; pass `--token-host <host>`"
            .to_string(),
        UnboundToken::StdinToken { variable_set: true } => format!(
            "the token on stdin was not sent because {WEB_TOKEN_HOST_ENV} binds only \
             {WEB_TOKEN_ENV}; pass `--token-host <host>` with the host it was issued for"
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    fn bare<'a>(argv: &[&str], target: &'a str) -> Invocation<'a> {
        let argv: Vec<OsString> = argv.iter().map(OsString::from).collect();
        Invocation {
            argv: Box::leak(argv.into_boxed_slice()),
            target,
            web_typed: false,
            quiet_env: false,
        }
    }

    fn kept(bound: &str, endpoint: Option<&str>, followed: bool) -> String {
        token_not_sent_message(
            &TokenNotSent {
                bound_host: Some(bound.to_string()),
                endpoint: endpoint.map(str::to_string),
                followed,
            },
            None,
        )
    }

    fn unbound(why: &UnboundToken) -> String {
        token_not_sent_message(
            &TokenNotSent {
                bound_host: None,
                endpoint: Some("https://evil.example/mcp".to_string()),
                followed: true,
            },
            Some(why),
        )
    }

    #[test]
    fn a_bound_token_kept_says_where_it_is_bound_and_what_the_run_found() {
        assert_eq!(
            kept("mcp.a.example", Some("https://a.example/mcp"), false),
            "the token was not sent: it is bound to mcp.a.example, and the audited site's own \
             MCP endpoint https://a.example/mcp outranks any endpoint the site declares on \
             another host"
        );
        assert_eq!(
            kept("mcp.a.example", Some("https://b.example/mcp"), true),
            "the token was not sent: it is bound to mcp.a.example, and the MCP endpoint is \
             https://b.example/mcp, on another host"
        );
        assert_eq!(
            kept("mcp.a.example", None, false),
            "the token was not sent: it is bound to mcp.a.example, and this run has no MCP \
             endpoint there"
        );
    }

    #[test]
    fn an_unbound_token_kept_names_the_channel_that_binds_it() {
        assert_eq!(
            unbound(&UnboundToken::EnvToken { flag_given: false }),
            "ANC_WEB_TOKEN was not sent because ANC_WEB_TOKEN_HOST does not name the host it was \
             issued for; set that variable, or pipe the token to `--token-stdin --token-host \
             <host>`"
        );
        assert_eq!(
            unbound(&UnboundToken::EnvToken { flag_given: true }),
            "ANC_WEB_TOKEN was not sent because --token-host alone does not bind it, and an \
             audited site can suggest that flag; set ANC_WEB_TOKEN_HOST to the host the token \
             was issued for"
        );
        assert_eq!(
            unbound(&UnboundToken::HostsDisagree {
                variable: "x.dev".to_string(),
                flag: "y.dev".to_string(),
            }),
            "ANC_WEB_TOKEN was not sent because ANC_WEB_TOKEN_HOST names x.dev and --token-host \
             names y.dev; the two must name the same host"
        );
        assert_eq!(
            unbound(&UnboundToken::StdinToken {
                variable_set: false
            }),
            "the token on stdin was not sent because --token-host does not name the host it was \
             issued for; pass `--token-host <host>`"
        );
        assert_eq!(
            unbound(&UnboundToken::StdinToken { variable_set: true }),
            "the token on stdin was not sent because ANC_WEB_TOKEN_HOST binds only \
             ANC_WEB_TOKEN; pass `--token-host <host>` with the host it was issued for"
        );
    }

    #[test]
    fn a_kept_token_offers_no_host_the_audited_site_chose() {
        for message in [
            unbound(&UnboundToken::EnvToken { flag_given: false }),
            unbound(&UnboundToken::EnvToken { flag_given: true }),
            unbound(&UnboundToken::StdinToken { variable_set: true }),
            unbound(&UnboundToken::StdinToken {
                variable_set: false,
            }),
        ] {
            assert!(!message.contains("evil.example"), "{message}");
        }
    }

    #[test]
    fn a_kept_token_prints_no_password_from_the_endpoint() {
        let endpoint = Some("https://user:secret@mcp.example.com/mcp");
        for message in [
            kept("x.dev", endpoint, true),
            kept("x.dev", endpoint, false),
        ] {
            assert!(!message.contains("secret"), "{message}");
        }
    }

    #[test]
    fn a_warning_prints_no_password_from_the_target() {
        let target = "https://user:secret@anc.dev";
        for message in [
            routed_to_web_message(&bare(&["anc", target], target)),
            token_not_read_message("ANC_WEB_TOKEN", &bare(&["anc", target], target)),
        ] {
            assert!(!message.contains("secret"), "{message}");
        }
    }

    #[test]
    fn the_token_notice_names_the_command_the_caller_ran() {
        let target = "internal.example.com";
        assert_eq!(
            token_not_read_message(
                "ANC_WEB_TOKEN",
                &bare(&["anc", target, "--no-external-dns", "--no-follow"], target)
            ),
            "ANC_WEB_TOKEN is set and not read: only \
             `anc web internal.example.com --no-external-dns --no-follow` with \
             ANC_WEB_TOKEN_HOST set sends it"
        );
    }

    #[test]
    fn a_warning_line_leads_with_its_code() {
        assert_eq!(
            warning_line(WarningCode::RoutedToWeb, "message"),
            "warning: routed-to-web: message"
        );
        assert_eq!(
            warning_line(WarningCode::NonComparable, "message"),
            "warning: non-comparable: message"
        );
        assert_eq!(
            warning_line(WarningCode::TokenNotSent, "message"),
            "warning: token-not-sent: message"
        );
    }

    #[test]
    fn a_bare_name_is_told_its_path_form_and_a_network_form_is_not() {
        assert_eq!(
            routed_to_web_message(&bare(&["anc", "anc.dev"], "anc.dev")),
            "`anc anc.dev` ran as `anc web anc.dev`; `anc audit ./anc.dev` audits a path by that name"
        );
        assert_eq!(
            routed_to_web_message(&bare(&["anc", "localhost:8787"], "localhost:8787")),
            "`anc localhost:8787` ran as `anc web localhost:8787`"
        );
        assert_eq!(
            routed_to_web_message(&bare(&["anc", "https://anc.dev"], "https://anc.dev")),
            "`anc https://anc.dev` ran as `anc web https://anc.dev`"
        );
        assert_eq!(
            routed_to_web_message(&bare(
                &["anc", "x.dev", "--no-external-dns", "--token-stdin"],
                "x.dev"
            )),
            "`anc x.dev --no-external-dns --token-stdin` ran as \
             `anc web x.dev --no-external-dns --token-stdin`; \
             `anc audit ./x.dev` audits a path by that name"
        );
    }
}
