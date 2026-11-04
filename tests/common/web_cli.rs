//! Shared scaffolding for the `anc web` test binaries: the command under
//! test, and the fixture sites it audits.

use std::io::Write as _;

use agentnative::web_audit::registry::CHECKS;
use assert_cmd::Command;

use super::{RawRequest, RawResponse, Server};

/// The checks this build cannot run, each bound to a handler kind or eval rule
/// the engine has not ported.
pub fn unported_checks() -> Vec<&'static str> {
    CHECKS
        .iter()
        .filter(|c| !c.is_ported())
        .map(|c| c.id)
        .collect()
}

/// A test gives up only its verdict, and says so past libtest's capture:
/// `scripts/web-audit-bump.sh` lists these lines in the bump PR, so a stand-down
/// cannot pass unseen.
fn stand_down(what: &str, why: &str) {
    let test = std::thread::current();
    let _ = writeln!(
        std::io::stderr(),
        "standing down: {}: {what}, because {why}",
        test.name().unwrap_or("a test"),
    );
}

/// A check this build cannot run skips wherever it applies, which makes that full
/// run "could not check" whatever else it found. Where the site's type or an
/// antecedent rules the check out, its row is N/A and the run keeps its verdict. The
/// ledger holds the gap and the release gate refuses a release with it.
fn stand_down_for_unported(what: &str) {
    stand_down(
        what,
        &format!(
            "of checks bound Unsupported: {}",
            unported_checks().join(", ")
        ),
    );
}

/// The exit codes a full run may end with: `code`, and 3 as well while a check is
/// unported.
pub fn full_run_code(code: i32) -> Vec<i32> {
    if unported_checks().is_empty() {
        return vec![code];
    }
    stand_down_for_unported(&format!("a full run may exit 3, not {code}"));
    vec![code, 3]
}

/// Whether a full run's exit code can be a verdict on the site.
pub fn ended_in_a_verdict(code: Option<i32>) -> bool {
    if unported_checks().is_empty() {
        return matches!(code, Some(0..=2));
    }
    stand_down_for_unported("a full run may exit 3, not a verdict");
    matches!(code, Some(0..=3))
}

/// Whether a full run's verdict and closing lines say what the rows earned.
pub fn verdict_is_pinned() -> bool {
    if unported_checks().is_empty() {
        return true;
    }
    stand_down_for_unported("the verdict and closing lines may read could not check");
    false
}

/// Whether a run over a corpus scenario earns its verdict: every check runs, and the
/// scenario itself reproduces.
pub fn scenario_verdict_is_pinned(scenario: &str, ledgered: bool) -> bool {
    if ledgered {
        stand_down(
            "the score is not pinned",
            &format!("{scenario} has ledger entries"),
        );
        return false;
    }
    verdict_is_pinned()
}

/// The binary, with the test CA standing in for the OS trust store:
/// rustls-native-certs reads `SSL_CERT_FILE` in place of the platform
/// store, and every fixture site is a local target, which trusts that
/// store. A test that needs the machine's own store removes the variable.
pub fn anc() -> Command {
    let mut cmd = Command::cargo_bin("anc").expect("binary should exist");
    cmd.env("SSL_CERT_FILE", super::test_ca_path())
        .env_remove("SSL_CERT_DIR");
    cmd
}

pub const HTML_HEAD: &str = "<!doctype html><html><head>\
<meta name=\"description\" content=\"A fixture site for the web-audit CLI tests.\">\
<link rel=\"alternate\" type=\"text/markdown\" href=\"/index.md\">\
</head><body><main><h1>Fixture</h1><p>";
pub const HTML_TAIL: &str =
    "</p></main><noscript><a href=\"/llms.txt\">llms.txt</a></noscript></body></html>";

pub fn html_body() -> String {
    let prose = "Readable prose about the fixture service and its agent surfaces. ".repeat(6);
    format!("{HTML_HEAD}{prose}{HTML_TAIL}")
}

pub fn ok(content_type: &str, body: &str) -> RawResponse {
    RawResponse::new(200, &[("content-type", content_type)], body.as_bytes())
}

pub fn not_found() -> RawResponse {
    RawResponse::new(
        404,
        &[("content-type", "text/html; charset=utf-8")],
        b"<html><body><h1>Not found</h1></body></html>",
    )
}

/// A site that publishes llms.txt and robots.txt and serves HTML at the
/// root: every MUST applicable to it passes, and a handful of SHOULDs miss.
/// It serves HTTPS with a leaf the test CA signed, which [`anc`] trusts.
pub fn fixture_site() -> Server {
    super::spawn_https(fixture_page)
}

/// [`fixture_site`] over plain HTTP, for proving no plaintext request
/// reaches it.
pub fn plaintext_site() -> Server {
    super::spawn(fixture_page)
}

fn fixture_page(req: &RawRequest) -> RawResponse {
    let path = req.target.split('?').next().unwrap_or("/");
    match path {
        "/" => ok("text/html; charset=utf-8", &html_body()),
        "/llms.txt" => ok(
            "text/plain; charset=utf-8",
            "# Fixture\n\n> A fixture site for the audit.\n\n## When to use\n\n- [Home](/)\n",
        ),
        "/robots.txt" => ok("text/plain", "User-agent: *\nAllow: /\n"),
        _ => not_found(),
    }
}

/// A site whose root is a MUST-bearing API surface that answers wrongly,
/// so the run has a real failure to report.
pub fn api_site() -> Server {
    super::spawn_https(|req: &RawRequest| {
        let path = req.target.split('?').next().unwrap_or("/");
        match path {
            "/" => ok("text/html; charset=utf-8", &html_body()),
            // A declared API with no OpenAPI document: openapi is a MUST.
            "/llms.txt" => ok(
                "text/plain; charset=utf-8",
                "# API fixture\n\n> An API.\n\n- [API](/api/v1/things)\n",
            ),
            _ => not_found(),
        }
    })
}

pub fn host_of(server: &Server) -> String {
    server.addr.to_string()
}

pub fn stdout_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

pub fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
