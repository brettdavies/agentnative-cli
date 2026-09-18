//! Shared scaffolding for the `anc web` test binaries: the command under
//! test, and the fixture sites it audits.

use assert_cmd::Command;

use super::{RawRequest, RawResponse, Server};

pub fn anc() -> Command {
    Command::cargo_bin("anc").expect("binary should exist")
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
pub fn fixture_site() -> Server {
    super::spawn(|req: &RawRequest| {
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
    })
}

/// A site whose root is a MUST-bearing API surface that answers wrongly,
/// so the run has a real failure to report.
pub fn api_site() -> Server {
    super::spawn(|req: &RawRequest| {
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
