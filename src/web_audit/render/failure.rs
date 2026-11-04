//! A run that ends without a scorecard: the reason, the hints, the next
//! step, and the exit code it returns.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::Path;

use super::exit::{EXIT_COULD_NOT_CHECK, EXIT_FAILURES};
use super::invocation::Invocation;
use crate::argv::format_invocation;
use crate::json_error::NextStep;
use crate::spawn_env::{WEB_TOKEN_ENV, WEB_TOKEN_HOST_ENV};
use crate::web_audit::credential::{TokenError, TokenSource};
use crate::web_audit::fetch::NOT_HTTPS;
use crate::web_audit::fetch::trust::CaCertError;
use crate::web_audit::target::{bare_name_path_form, redact_userinfo};
use crate::web_audit::token_host::{InvalidTokenHost, TokenHostSource};
use crate::web_audit::transport::ROOT_REJECTION_PREFIX;

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
pub(super) const TOKEN_STDIN_ARGUMENT: &str = "--token-stdin";
/// The `--token-host` flag, as clap names it.
const TOKEN_HOST_ARGUMENT: &str = "--token-host <HOST>";

/// `anc web` with `args`, shell-quoted so the line runs as printed.
pub(super) fn web_command(args: &[&str]) -> String {
    let argv: Vec<OsString> = ["anc", "web"]
        .iter()
        .chain(args)
        .map(OsString::from)
        .collect();
    format_invocation(&argv)
}

/// `anc audit` on the path a bare name would be if it is not a host, or `None`
/// for a token that names a network target outright.
pub(super) fn audit_path_command(token: &str) -> Option<String> {
    let path = bare_name_path_form(token)?;
    let argv: Vec<OsString> = ["anc", "audit", path.as_str()]
        .iter()
        .map(OsString::from)
        .collect();
    Some(format_invocation(&argv))
}

/// The failure a run ends on when its target gave nothing to score, chosen by
/// the engine's root error. A plaintext refusal and a chain no root vouches for
/// are their own dead ends: retrying changes neither, and each names its fix.
/// A sniffed bare name can be a mistyped path, and the message is what an agent
/// reads in JSON mode, so it says that there.
#[must_use]
pub fn no_answer_failure(
    root_error: &str,
    reason: &str,
    invocation: &Invocation<'_>,
    normalized: &str,
    defaulted_https: bool,
) -> Failure {
    let typed = redact_userinfo(invocation.target);
    let redirected =
        root_error.starts_with("redirect refused: ") && root_error.ends_with(": not https");
    let mut failure = if root_error == NOT_HTTPS {
        let https = normalized.replacen("http://", "https://", 1);
        insecure_scheme_failure(normalized, reason, Some(invocation.on(&https)))
    } else if redirected {
        insecure_scheme_failure(normalized, reason, None)
    } else if root_error.starts_with(ROOT_REJECTION_PREFIX) {
        untrusted_certificate_failure(normalized, root_error, invocation.ca_cert_template(&typed))
    } else {
        unreachable_failure(normalized, defaulted_https, invocation.retry(&typed))
    };
    if !invocation.web_typed && bare_name_path_form(&typed).is_some() {
        let _ = write!(
            failure.message,
            "; if you meant a local path, {typed} does not exist"
        );
    }
    if invocation.gave_token_stdin() && !redirected {
        failure.message.push_str(
            "; the next step leaves out --token-stdin, because a printed command cannot carry \
             stdin",
        );
    }
    failure
}

/// The target answered nothing. The engine's reason is the hosted
/// auditor's wording, which speaks for its own vantage point, so the
/// local run says what is true here instead: nothing on this machine's
/// network path answered.
fn unreachable_failure(normalized: &str, defaulted_https: bool, retry: String) -> Failure {
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
        next_step: NextStep::retry(retry),
        exit_code: EXIT_COULD_NOT_CHECK,
        argument: TARGET_ARGUMENT,
        value: normalized.to_string(),
    }
}

/// The target answered with a certificate chain no root in the trust set
/// vouches for. Nothing was sent past the handshake, and retrying changes
/// nothing; trusting the server's CA does.
fn untrusted_certificate_failure(normalized: &str, evidence: &str, template: String) -> Failure {
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
        next_step: NextStep::add_ca_cert(template),
        exit_code: EXIT_COULD_NOT_CHECK,
        argument: TARGET_ARGUMENT,
        value: normalized.to_string(),
    }
}

/// The target is `http`, or its root redirects to `http`: the run ends
/// before any plaintext request, with the engine's no-plaintext reason as
/// the message. `https_command` is the same command on the `https` URL, for
/// an `http` target; a root that redirects to `http` has no form of the
/// command that gets past it.
fn insecure_scheme_failure(
    normalized: &str,
    reason: &str,
    https_command: Option<String>,
) -> Failure {
    let cause = if https_command.is_none() {
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
        next_step: NextStep::fix_target(https_command.unwrap_or_else(|| web_command(&["--help"]))),
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
    let shown = redact_userinfo(token);
    Failure {
        slug: "invalid-target",
        message: format!("cannot audit {shown}: {reason}"),
        hints: vec![
            "pass a URL or a host, such as `anc web localhost:8787` or `anc web https://anc.dev`"
                .to_string(),
        ],
        next_step: NextStep::fix_target(web_command(&["--help"])),
        exit_code: EXIT_COULD_NOT_CHECK,
        argument: TARGET_ARGUMENT,
        value: shown,
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

/// A named token host is not a URL host, a usage error raised before any
/// request goes out. Userinfo stays out of what it echoes.
pub fn token_host_failure(error: &InvalidTokenHost) -> Failure {
    let (argument, source) = match error.source {
        TokenHostSource::Flag => (TOKEN_HOST_ARGUMENT, "--token-host"),
        TokenHostSource::Env => (WEB_TOKEN_HOST_ENV, WEB_TOKEN_HOST_ENV),
    };
    let redacted = redact_userinfo(&error.value);
    let shown = redacted
        .rsplit_once('@')
        .map_or(redacted.as_str(), |(_, host)| host);
    Failure {
        slug: "invalid-token-host",
        message: format!("{source} `{shown}` is not a URL host"),
        hints: vec![
            "name the MCP endpoint's host, with a port when it is not 443, and no scheme, path, \
             query, or userinfo: `mcp.example.com` or `localhost:8787`"
                .to_string(),
        ],
        next_step: NextStep::show_help(web_command(&["--help"])),
        exit_code: EXIT_FAILURES,
        argument,
        value: shown.to_string(),
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

    fn typed(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn command(failure: &Failure) -> String {
        failure.next_step.command.clone().unwrap_or_default()
    }

    /// The failure an invocation ends on: `root_error`, with `argv`'s first word
    /// after the program deciding whether `web` was typed.
    fn ended(root_error: &str, target: &str, normalized: &str, argv: &[&str]) -> Failure {
        let argv = typed(argv);
        no_answer_failure(
            root_error,
            "the reason",
            &Invocation {
                argv: &argv,
                target,
                web_typed: argv[1] == "web",
                quiet_env: false,
            },
            normalized,
            !target.contains("://"),
        )
    }

    #[test]
    fn a_retry_command_quotes_a_target_a_shell_would_split() {
        let target = "http://127.0.0.1:9/?a=1&b=2";
        let failure = ended("refused", target, target, &["anc", "web", target]);
        assert_eq!(
            command(&failure),
            "anc web 'http://127.0.0.1:9/?a=1&b=2' --verbose"
        );
    }

    #[test]
    fn no_answer_picks_the_failure_from_the_root_error_and_how_the_target_arrived() {
        let pick = |root_error: &str, target: &str, normalized: &str, web_typed: bool| {
            if web_typed {
                ended(root_error, target, normalized, &["anc", "web", target])
            } else {
                ended(root_error, target, normalized, &["anc", target])
            }
        };
        const PATH_READING: &str = "if you meant a local path, missing.dev does not exist";

        let sniffed = pick("refused", "missing.dev", "https://missing.dev/", false);
        assert_eq!(sniffed.slug, "target-unreachable");
        assert!(
            sniffed.message.contains(PATH_READING),
            "{}",
            sniffed.message
        );
        assert_eq!(command(&sniffed), "anc missing.dev --verbose");

        let named = pick("refused", "missing.dev", "https://missing.dev/", true);
        assert!(!named.message.contains("local path"), "{}", named.message);
        assert_eq!(command(&named), "anc web missing.dev --verbose");

        let port = pick(
            "refused",
            "localhost:8787",
            "https://localhost:8787/",
            false,
        );
        assert!(!port.message.contains("local path"), "{}", port.message);
        assert_eq!(command(&port), "anc localhost:8787 --verbose");

        let scheme = pick(
            "refused",
            "https://missing.dev",
            "https://missing.dev/",
            false,
        );
        assert!(!scheme.message.contains("local path"), "{}", scheme.message);

        let plaintext = pick(NOT_HTTPS, "http://x.dev", "http://x.dev/", true);
        assert_eq!(plaintext.slug, "insecure-scheme");
        assert_eq!(command(&plaintext), "anc web https://x.dev/");
        let bare_plaintext = pick(NOT_HTTPS, "http://x.dev", "http://x.dev/", false);
        assert_eq!(command(&bare_plaintext), "anc https://x.dev/");

        let redirected = pick(
            "redirect refused: http://x.dev/: not https",
            "x.dev",
            "https://x.dev/",
            true,
        );
        assert_eq!(redirected.slug, "insecure-scheme");
        assert_eq!(command(&redirected), "anc web --help");
        assert!(redirected.hints[0].contains("redirects https to http"));

        let rejected = format!("{ROOT_REJECTION_PREFIX} the chain");
        let untrusted = pick(&rejected, "x.dev", "https://x.dev/", true);
        assert_eq!(untrusted.slug, "untrusted-certificate");
        assert_eq!(
            untrusted.next_step.template.as_deref(),
            Some("anc web x.dev --ca-cert <FILE>")
        );
        assert!(
            !untrusted.message.contains("local path"),
            "{}",
            untrusted.message
        );
        let bare_untrusted = pick(&rejected, "x.dev", "https://x.dev/", false);
        assert!(
            bare_untrusted
                .message
                .contains("if you meant a local path, x.dev does not exist"),
            "{}",
            bare_untrusted.message
        );
        let bare_redirect = pick(
            "redirect refused: http://x.dev/: not https",
            "x.dev",
            "https://x.dev/",
            false,
        );
        assert!(
            bare_redirect
                .message
                .contains("if you meant a local path, x.dev does not exist"),
            "{}",
            bare_redirect.message
        );
    }

    #[test]
    fn a_next_step_keeps_the_flags_it_was_typed_with() {
        let target = "127.0.0.1:9";
        let argv = [
            "anc",
            "web",
            target,
            "--no-external-dns",
            "--no-follow",
            "-q",
            "--output",
            "json",
        ];
        let failure = ended("refused", target, "https://127.0.0.1:9/", &argv);
        assert_eq!(
            command(&failure),
            "anc web 127.0.0.1:9 --no-external-dns --no-follow --output json --verbose"
        );

        let failure = ended(
            "refused",
            target,
            "https://127.0.0.1:9/",
            &["anc", "web", target, "-v"],
        );
        assert_eq!(command(&failure), "anc web 127.0.0.1:9 -v");
    }

    #[test]
    fn a_next_step_runs_as_printed() {
        let target = "127.0.0.1:9";
        let normalized = "https://127.0.0.1:9/";
        let rejected = format!("{ROOT_REJECTION_PREFIX} the chain");

        // A flag after `--` is a positional, and clap refuses a second one.
        let dashes = ["anc", "web", "--", target];
        assert_eq!(
            command(&ended("refused", target, normalized, &dashes)),
            "anc web --verbose -- 127.0.0.1:9"
        );
        assert_eq!(
            ended(&rejected, target, normalized, &dashes)
                .next_step
                .template
                .as_deref(),
            Some("anc web --ca-cert <FILE> -- 127.0.0.1:9")
        );

        // `--token-stdin` on a terminal exits 2, so a printed line leaves it out,
        // and the message is what says so in JSON mode too.
        let stdin = ["anc", "web", target, "--token-stdin"];
        let failure = ended("refused", target, normalized, &stdin);
        assert_eq!(command(&failure), "anc web 127.0.0.1:9 --verbose");
        assert!(
            failure.message.contains("--token-stdin"),
            "{}",
            failure.message
        );
        assert_eq!(
            ended(&rejected, target, normalized, &stdin)
                .next_step
                .template
                .as_deref(),
            Some("anc web 127.0.0.1:9 --ca-cert <FILE>")
        );
        let redirect = "redirect refused: http://x/: not https";
        let help = ended(redirect, target, normalized, &stdin);
        assert_eq!(command(&help), "anc web --help");
        assert!(!help.message.contains("--token-stdin"), "{}", help.message);
        let plain = ended("refused", target, normalized, &["anc", "web", target]);
        assert!(
            !plain.message.contains("--token-stdin"),
            "{}",
            plain.message
        );

        // The variable conflicts with `--verbose` whatever its value.
        let argv = typed(&["anc", "web", target, "-q"]);
        let failure = no_answer_failure(
            "refused",
            "reason",
            &Invocation {
                argv: &argv,
                target,
                web_typed: true,
                quiet_env: true,
            },
            normalized,
            true,
        );
        assert_eq!(command(&failure), "anc web 127.0.0.1:9");
    }

    #[test]
    fn a_failure_prints_no_password_from_the_target() {
        let target = "https://user:secret@127.0.0.1:9/x";
        let normalized = crate::web_audit::target::normalize_target(target).unwrap();
        for root_error in ["refused", ROOT_REJECTION_PREFIX, NOT_HTTPS] {
            let failure = ended(root_error, target, &normalized, &["anc", "web", target]);
            assert!(!failure.message.contains("secret"), "{}", failure.message);
            assert!(!failure.value.contains("secret"), "{}", failure.value);
            let printed = format!("{:?}", failure.next_step);
            assert!(!printed.contains("secret"), "{printed}");
            assert!(printed.contains("127.0.0.1:9/x"), "{printed}");
        }

        let target = "user:secret@staging.internal:8443";
        let failure = ended(
            "refused",
            target,
            "https://staging.internal:8443/",
            &["anc", "web", target],
        );
        assert_eq!(command(&failure), "anc web staging.internal:8443 --verbose");
        assert!(!failure.value.contains("secret"), "{}", failure.value);

        let failure = invalid_target_failure("ftp://user:secret@host/", "the reason");
        assert!(!failure.message.contains("secret"), "{}", failure.message);
        assert_eq!(failure.value, "ftp://host/");
    }

    #[test]
    fn the_failure_trailer_names_what_its_exit_code_means() {
        let unknown = format_failure(&unknown_check_failure("no-such-check"));
        assert!(unknown.ends_with("\nexit 2: usage error\n"), "{unknown}");
        let unreachable = format_failure(&ended(
            "refused",
            "127.0.0.1:9",
            "http://127.0.0.1:9/",
            &["anc", "web", "127.0.0.1:9"],
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
        assert_eq!(spelled("token_host"), TOKEN_HOST_ARGUMENT);
    }
}
