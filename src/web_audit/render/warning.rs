//! A web run's stderr warnings, each `warning: <code>: <message>`, so a
//! consumer can branch on the code and a person can read the message.

use super::failure::audit_path_command;
use super::invocation::Invocation;
use crate::web_audit::target::redact_userinfo;

/// The closed set of codes a web run's stderr warning carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarningCode {
    /// The run hit a check this build cannot score, so its score does not
    /// compare with anc.dev's.
    NonComparable,
    /// A bare `anc <target>` ran as `anc web <target>`.
    RoutedToWeb,
}

impl WarningCode {
    /// The code as it prints.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NonComparable => "non-comparable",
            Self::RoutedToWeb => "routed-to-web",
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
        "{env_name} is set and not read: only `{}` sends it",
        invocation.naming_web(&redact_userinfo(invocation.target))
    )
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
             `anc web internal.example.com --no-external-dns --no-follow` sends it"
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
