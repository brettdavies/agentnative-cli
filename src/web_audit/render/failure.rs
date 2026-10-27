//! A run that ends without a scorecard: the reason, the hints, the next
//! step, and the exit code it returns.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::Path;

use super::exit::{EXIT_COULD_NOT_CHECK, EXIT_FAILURES};
use crate::argv::format_invocation;
use crate::json_error::NextStep;
use crate::spawn_env::WEB_TOKEN_ENV;
use crate::web_audit::credential::{TokenError, TokenSource};
use crate::web_audit::fetch::trust::CaCertError;

/// A run that ended without a scorecard, and what to do about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// The stable, kebab-case reason an agent branches on.
    pub slug: &'static str,
    /// What went wrong, in one sentence.
    pub message: String,
    /// The cause and what to try, one line each.
    pub hints: Vec<String>,
    /// The action, command and docs an agent takes next.
    pub next_step: NextStep,
    /// The exit code this failure returns.
    pub exit_code: i32,
    /// The input the failure names, spelled as clap's own errors spell it:
    /// `<TARGET>`, `--check <ID>` or `--ca-cert <FILE>`.
    pub argument: &'static str,
    /// The value of [`Failure::argument`] the failure is about.
    pub value: String,
}

/// The positional `anc web` audits, as clap names it.
const TARGET_ARGUMENT: &str = "<TARGET>";
/// The `--check` flag, as clap names it.
const CHECK_ARGUMENT: &str = "--check <ID>";
/// The `--ca-cert` flag, as clap names it.
const CA_CERT_ARGUMENT: &str = "--ca-cert <FILE>";
/// The `--token-stdin` flag, as clap names it.
const TOKEN_STDIN_ARGUMENT: &str = "--token-stdin";

/// `anc web` with `args`, shell-quoted so the line runs as printed.
fn web_command(args: &[&str]) -> String {
    let argv: Vec<OsString> = ["anc", "web"]
        .iter()
        .chain(args)
        .map(OsString::from)
        .collect();
    format_invocation(&argv)
}

/// The target answered nothing. The engine's reason is the hosted
/// auditor's wording, which speaks for its own vantage point, so the
/// local run says what is true here instead: nothing on this machine's
/// network path answered.
pub fn unreachable_failure(token: &str, normalized: &str, defaulted_https: bool) -> Failure {
    let mut hints = vec![format!(
        "no HTTP response from {normalized}, on the root fetch or on any MCP discovery probe"
    )];
    if defaulted_https {
        hints.push(
            "the scheme defaulted to https; anc web sends no plaintext request, so a server \
             that speaks only HTTP cannot be audited until it serves TLS"
                .to_string(),
        );
    }
    hints.push("check that the server is running and that this machine can reach it".to_string());
    Failure {
        slug: "target-unreachable",
        message: format!("{normalized} did not answer any probe"),
        hints,
        next_step: NextStep::retry(web_command(&[token, "--verbose"])),
        exit_code: EXIT_COULD_NOT_CHECK,
        argument: TARGET_ARGUMENT,
        value: normalized.to_string(),
    }
}

/// The target answered with a certificate chain no root in the trust set
/// vouches for. Nothing was sent past the handshake, and retrying changes
/// nothing; trusting the server's CA does.
pub fn untrusted_certificate_failure(token: &str, normalized: &str, evidence: &str) -> Failure {
    Failure {
        slug: "untrusted-certificate",
        message: format!("{normalized} presented a certificate chain this run does not trust"),
        hints: vec![
            evidence.to_string(),
            "if the server's certificate comes from a private or development CA, pass that \
             CA's PEM certificate with `--ca-cert <FILE>`, or install it in the OS trust store, \
             which a local or private target also trusts"
                .to_string(),
        ],
        next_step: NextStep::add_ca_cert(format!("{} --ca-cert <FILE>", web_command(&[token]))),
        exit_code: EXIT_COULD_NOT_CHECK,
        argument: TARGET_ARGUMENT,
        value: normalized.to_string(),
    }
}

/// The target is `http`, or its root redirects to `http`: the run ends
/// before any plaintext request, with the engine's no-plaintext reason as
/// the message.
pub fn insecure_scheme_failure(normalized: &str, reason: &str, redirected: bool) -> Failure {
    let cause = if redirected {
        "the root redirects https to http; serve the site over https end to end"
    } else {
        "anc web sends HTTPS only, local targets included: serve the site over TLS and audit \
         its https URL"
    };
    Failure {
        slug: "insecure-scheme",
        message: reason.to_string(),
        hints: vec![
            cause.to_string(),
            "a certificate from a private or development CA needs `--ca-cert <FILE>`, or that \
             CA in the OS trust store"
                .to_string(),
        ],
        next_step: NextStep::fix_target(web_command(&["--help"])),
        exit_code: EXIT_COULD_NOT_CHECK,
        argument: TARGET_ARGUMENT,
        value: normalized.to_string(),
    }
}

/// A `--ca-cert` file supplies no usable root, a usage error raised before
/// any request goes out.
pub fn ca_cert_failure(path: &Path, reason: &CaCertError) -> Failure {
    let shown = path.display().to_string();
    Failure {
        slug: "invalid-ca-cert",
        message: format!("cannot trust --ca-cert {shown}: {reason}"),
        hints: vec![
            "pass a PEM file holding the CA's root certificate, a block that begins \
             `-----BEGIN CERTIFICATE-----`"
                .to_string(),
        ],
        next_step: NextStep::show_help(web_command(&["--help"])),
        exit_code: EXIT_FAILURES,
        argument: CA_CERT_ARGUMENT,
        value: shown,
    }
}

/// The target could not be read as an http(s) URL at all.
pub fn invalid_target_failure(token: &str, reason: &str) -> Failure {
    Failure {
        slug: "invalid-target",
        message: format!("cannot audit {token}: {reason}"),
        hints: vec![
            "pass a URL or a host, such as `anc web localhost:8787` or `anc web https://anc.dev`"
                .to_string(),
        ],
        next_step: NextStep::fix_target(web_command(&["--help"])),
        exit_code: EXIT_COULD_NOT_CHECK,
        argument: TARGET_ARGUMENT,
        value: token.to_string(),
    }
}

/// `--check` named an id the compiled registry does not carry, which is a
/// usage error rather than an unreachable surface.
pub fn unknown_check_failure(id: &str) -> Failure {
    Failure {
        slug: "unknown-check",
        message: format!("no such check: {id}"),
        hints: vec!["run `anc emit web-checks` for every id this build scores".to_string()],
        next_step: NextStep::list_checks("anc emit web-checks".to_string()),
        exit_code: EXIT_FAILURES,
        argument: CHECK_ARGUMENT,
        value: id.to_string(),
    }
}

/// No usable token was read, a usage error raised before any request
/// goes out. The value is never echoed: `value` is empty for an empty
/// token and `[redacted]` for one that was read.
pub fn token_failure(error: &TokenError) -> Failure {
    let (argument, source) = match error {
        TokenError::Empty(TokenSource::Env) | TokenError::Invalid(TokenSource::Env) => {
            (WEB_TOKEN_ENV, WEB_TOKEN_ENV)
        }
        _ => (TOKEN_STDIN_ARGUMENT, "stdin"),
    };
    let (slug, message, hint, value) = match error {
        TokenError::Empty(_) => (
            "empty-token",
            format!("the token from {source} is empty"),
            "pass the MCP endpoint's bearer token, or leave ANC_WEB_TOKEN unset to audit \
             without signing in",
            "",
        ),
        TokenError::Invalid(_) => (
            "invalid-token",
            format!("the token from {source} is not a bearer token"),
            "a bearer token is visible ASCII with no spaces; check that the whole token, \
             and nothing else, was passed",
            "[redacted]",
        ),
        TokenError::StdinTerminal => (
            "token-stdin-terminal",
            "--token-stdin reads the token from stdin, and stdin is a terminal".to_string(),
            "pipe the token in, for example `anc web <TARGET> --token-stdin < token.txt`",
            "",
        ),
        TokenError::StdinUnreadable(cause) => (
            "token-stdin-unreadable",
            format!("--token-stdin could not read stdin: {cause}"),
            "pipe the token in as one line of text",
            "",
        ),
    };
    Failure {
        slug,
        message,
        hints: vec![hint.to_string()],
        next_step: NextStep::show_help(web_command(&["--help"])),
        exit_code: EXIT_FAILURES,
        argument,
        value: value.to_string(),
    }
}

impl Failure {
    /// The envelope's `kind`: `usage` for a failure that exits as a usage
    /// error, `runtime` for one that could not check.
    pub fn kind(&self) -> &'static str {
        if self.exit_code == EXIT_FAILURES {
            "usage"
        } else {
            "runtime"
        }
    }
}

/// The text rendering of a failure: the problem, then the cause and the
/// next thing to try, then the code and what earned it.
pub fn format_failure(failure: &Failure) -> String {
    let mut out = format!("error: {}\n", failure.message);
    for hint in &failure.hints {
        let _ = writeln!(out, "  {hint}");
    }
    if let Some(command) = &failure.next_step.command {
        let _ = writeln!(out, "  next: {command}");
    }
    let _ = writeln!(
        out,
        "\nexit {}: {}",
        failure.exit_code,
        failure_meaning(failure.exit_code)
    );
    out
}

/// What a failure's exit code means, as the exit table words it. A run
/// that ends without a scorecard exits 2 only for a usage error.
fn failure_meaning(exit_code: i32) -> &'static str {
    match exit_code {
        EXIT_FAILURES => "usage error",
        _ => "could not check",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retry_command_quotes_a_target_a_shell_would_split() {
        let failure = unreachable_failure(
            "http://127.0.0.1:9/?a=1&b=2",
            "http://127.0.0.1:9/?a=1&b=2",
            false,
        );
        assert_eq!(
            failure.next_step.command.as_deref(),
            Some("anc web 'http://127.0.0.1:9/?a=1&b=2' --verbose")
        );
        let plain = unreachable_failure("127.0.0.1:9", "http://127.0.0.1:9/", false);
        assert_eq!(
            plain.next_step.command.as_deref(),
            Some("anc web 127.0.0.1:9 --verbose")
        );
    }

    #[test]
    fn the_failure_trailer_names_what_its_exit_code_means() {
        let unknown = format_failure(&unknown_check_failure("no-such-check"));
        assert!(unknown.ends_with("\nexit 2: usage error\n"), "{unknown}");
        let unreachable = format_failure(&unreachable_failure(
            "127.0.0.1:9",
            "http://127.0.0.1:9/",
            false,
        ));
        assert!(
            unreachable.ends_with("\nexit 3: could not check\n"),
            "{unreachable}"
        );
        let invalid = format_failure(&invalid_target_failure("ftp://x", "unsupported scheme"));
        assert!(
            invalid.ends_with("\nexit 3: could not check\n"),
            "{invalid}"
        );
    }

    #[test]
    fn a_failure_names_its_argument_as_clap_does() {
        let mut cli = <crate::cli::Cli as clap::CommandFactory>::command();
        cli.build();
        let web = cli.find_subcommand("web").expect("the web subcommand");
        let spelled = |id: &str| {
            web.get_arguments()
                .find(|arg| arg.get_id() == id)
                .unwrap_or_else(|| panic!("no `{id}` argument"))
                .to_string()
        };
        assert_eq!(spelled("target"), TARGET_ARGUMENT);
        assert_eq!(spelled("check"), CHECK_ARGUMENT);
        assert_eq!(spelled("ca_cert"), CA_CERT_ARGUMENT);
        assert_eq!(spelled("token_stdin"), TOKEN_STDIN_ARGUMENT);
    }
}
