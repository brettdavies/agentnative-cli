//! `anc web` with a sign-in token, end to end over HTTPS: the two inputs
//! (`ANC_WEB_TOKEN` and `--token-stdin`), the structured refusals of a
//! token that cannot be used, and the token's absence from every byte the
//! run prints and from every request but the endpoint's own.

#![cfg(feature = "web")]

mod common;

use common::web_cli::{anc, host_of, html_body, not_found, ok, stderr_of, stdout_of};
use common::{RawRequest, RawResponse, Server};
use serde_json::{Value, json};

const TOKEN: &str = "fixture-token-5c1d";

fn bearer() -> String {
    format!("Bearer {TOKEN}")
}

fn rpc(result: Value) -> RawResponse {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "result": result }).to_string();
    RawResponse::new(
        200,
        &[("content-type", "application/json")],
        body.as_bytes(),
    )
}

fn endpoint_answer(req: &RawRequest) -> RawResponse {
    let host = req.header("host").unwrap_or("127.0.0.1");
    if req.header("authorization") != Some(bearer().as_str()) {
        let challenge = format!(
            "Bearer resource_metadata=\"https://{host}/.well-known/oauth-protected-resource/mcp\""
        );
        return RawResponse::new(
            401,
            &[
                ("content-type", "application/json"),
                ("www-authenticate", &challenge),
            ],
            br#"{"error":"unauthorized"}"#,
        );
    }
    let method = serde_json::from_slice::<Value>(&req.body)
        .ok()
        .and_then(|v| v.get("method").and_then(Value::as_str).map(str::to_string));
    match method.as_deref() {
        Some("initialize") => rpc(json!({
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "fixture", "version": "1.0" },
        })),
        Some("tools/list") => rpc(json!({ "tools": [] })),
        Some("notifications/initialized") => RawResponse::new(202, &[], b""),
        _ => {
            let body =
                json!({ "jsonrpc": "2.0", "id": 1, "error": { "code": -32601, "message": "no" } });
            RawResponse::new(
                200,
                &[("content-type", "application/json")],
                body.to_string().as_bytes(),
            )
        }
    }
}

/// A site whose MCP endpoint, found on a common path, requires sign-in and
/// serves a client that presents [`TOKEN`].
fn protected_site() -> Server {
    common::spawn_https(|req: &RawRequest| {
        let host = req.header("host").unwrap_or("127.0.0.1");
        let path = req.target.split('?').next().unwrap_or("/");
        match (req.method.as_str(), path) {
            ("GET", "/") => ok("text/html; charset=utf-8", &html_body()),
            ("POST", "/mcp") => endpoint_answer(req),
            ("GET", "/.well-known/oauth-protected-resource/mcp") => ok(
                "application/json",
                &json!({
                    "resource": format!("https://{host}/mcp"),
                    "authorization_servers": ["https://auth.example.com"],
                })
                .to_string(),
            ),
            _ => not_found(),
        }
    })
}

fn row<'a>(card: &'a Value, id: &str) -> &'a Value {
    card["results"]
        .as_array()
        .and_then(|rows| rows.iter().find(|r| r["id"] == id))
        .unwrap_or_else(|| panic!("no row {id}"))
}

fn assert_no_token(output: &std::process::Output) {
    for (stream, text) in [("stdout", stdout_of(output)), ("stderr", stderr_of(output))] {
        assert!(!text.contains(TOKEN), "{stream} printed the token:\n{text}");
    }
}

/// Every request that carried the token went to the endpoint's POSTs, and
/// at least one did.
fn assert_only_the_endpoint_was_signed(server: &Server) {
    let hits = server.hits();
    let signed: Vec<&RawRequest> = hits
        .iter()
        .filter(|r| r.header("authorization").is_some())
        .collect();
    assert!(!signed.is_empty(), "nothing was signed: {hits:#?}");
    for request in signed {
        assert_eq!(
            (request.method.as_str(), request.target.as_str()),
            ("POST", "/mcp"),
            "the token left the endpoint"
        );
    }
    assert!(
        hits.iter()
            .any(|r| r.target.contains("oauth-protected-resource")),
        "the metadata was never read"
    );
}

#[test]
fn a_token_in_the_environment_signs_the_run_in_and_is_never_printed() {
    let server = protected_site();
    let assert = anc()
        .args(["web", &host_of(&server), "--output", "json"])
        .env("ANC_WEB_TOKEN", TOKEN)
        .assert();
    let output = assert.get_output();
    assert_no_token(output);
    let card: Value = serde_json::from_str(&stdout_of(output)).expect("a JSON scorecard");
    assert_eq!(
        card["vantage"],
        json!({ "network": "local", "credentialed": true })
    );
    for id in [
        "mcp-initialize",
        "mcp-tools-list",
        "mcp-auth-challenge",
        "mcp-auth-servers",
        "mcp-auth-enforced",
    ] {
        assert_eq!(row(&card, id)["status"], "pass", "{id}: {}", row(&card, id));
    }
    assert_only_the_endpoint_was_signed(&server);

    let text = anc()
        .args(["web", &host_of(&server), "--verbose"])
        .env("ANC_WEB_TOKEN", TOKEN)
        .assert();
    assert_no_token(text.get_output());

    for mode in ["json", "text"] {
        let dead_end = anc()
            .args(["web", "127.0.0.1:9", "--output", mode])
            .env("ANC_WEB_TOKEN", TOKEN)
            .assert()
            .code(3);
        assert_no_token(dead_end.get_output());
    }
}

#[test]
fn a_token_on_stdin_signs_the_run_in() {
    let server = protected_site();
    let assert = anc()
        .args([
            "web",
            &host_of(&server),
            "--output",
            "json",
            "--token-stdin",
        ])
        .write_stdin(format!("{TOKEN}\n"))
        .assert();
    let output = assert.get_output();
    assert_no_token(output);
    let card: Value = serde_json::from_str(&stdout_of(output)).expect("a JSON scorecard");
    assert_eq!(card["vantage"]["credentialed"], true);
    assert_eq!(row(&card, "mcp-tools-list")["status"], "pass");
    assert_only_the_endpoint_was_signed(&server);
}

#[test]
fn an_unusable_token_is_refused_before_any_request() {
    let server = protected_site();
    let cases: [(&[&str], Option<&str>, &str, &str); 3] = [
        (&[], Some(""), "empty-token", "ANC_WEB_TOKEN"),
        (&["--token-stdin"], None, "empty-token", "--token-stdin"),
        (&[], Some("two words"), "invalid-token", "ANC_WEB_TOKEN"),
    ];
    for (flags, env, slug, argument) in cases {
        let mut cmd = anc();
        cmd.args(["web", &host_of(&server), "--output", "json"])
            .args(flags)
            .env_remove("ANC_WEB_TOKEN")
            .write_stdin("\n");
        if let Some(value) = env {
            cmd.env("ANC_WEB_TOKEN", value);
        }
        let assert = cmd.assert().code(2);
        let output = assert.get_output();
        assert_eq!(stdout_of(output), "", "{slug}");
        let envelope: Value =
            serde_json::from_str(stderr_of(output).trim()).expect("a JSON error envelope");
        assert_eq!(envelope["error"], slug);
        assert_eq!(envelope["kind"], "usage");
        assert_eq!(envelope["exit_code"], 2);
        assert_eq!(envelope["argument"], argument);
        assert_eq!(envelope["next_step"]["action"], "show-help");
        assert!(!stderr_of(output).contains("two words"), "{envelope}");
    }
    assert!(
        server.hits().is_empty(),
        "a refused token still sent requests"
    );
}

#[test]
fn help_documents_both_token_inputs_and_where_the_token_goes() {
    let assert = anc().args(["web", "--help"]).assert().code(0);
    let help = stdout_of(assert.get_output());
    for needle in [
        "ANC_WEB_TOKEN=<token> anc web <target>",
        "--token-stdin",
        "goes only to the audited MCP endpoint's handshake and session",
    ] {
        assert!(help.contains(needle), "`anc web --help` lacks {needle:?}");
    }
}
