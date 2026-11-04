//! A token goes out only when its own channel names the URL host it belongs
//! to: `ANC_WEB_TOKEN_HOST` for `ANC_WEB_TOKEN`, `--token-host` for
//! `--token-stdin`. Over the corpus's recorded exchanges and over HTTPS end to
//! end, it reaches an endpoint of record on that host and nothing else, a
//! declared host only after it confirms the endpoint, and any other token is
//! kept and the run says so.

#![cfg(feature = "web")]

mod common;

use std::process::Output;
use std::sync::Arc;

use agentnative::web_audit::credential::{Credential, TokenSource};
use agentnative::web_audit::engine::{FetchHandle, RunInput, RunOutcome, RunReport, run_web_audit};
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::mock::{MockResponse, MockRule, MockTransport};
use agentnative::web_audit::registry::AUDIT_USER_AGENT;
use agentnative::web_audit::token_host::{TokenHost, TokenNotSent};
use agentnative::web_audit::transport::Request;
use common::corpus::{PublicResolver, load_cases, replay_as, transport_for};
use common::web_cli::{anc, host_of, html_body, not_found, ok, stderr_of, stdout_of};
use common::{RawRequest, RawResponse, Server};
use serde_json::{Value, json};

const TOKEN: &str = "token-issued-for-another-host";
const NOT_SENT: &str = "warning: token-not-sent: ";
const DECLARED_ENDPOINT: &str = "https://mcp.example.net/mcp";
const OWN_ENDPOINT: &str = "https://example.com/mcp";
const CARD_SCHEMA: &str =
    "https://static.modelcontextprotocol.io/schemas/v1/server-card.schema.json";

fn credential() -> Credential {
    Credential::bearer(TOKEN, TokenSource::Env).expect("a valid token")
}

fn token_host(raw: &str) -> TokenHost {
    TokenHost::parse(raw).unwrap_or_else(|| panic!("{raw} is a host"))
}

fn complete(outcome: RunOutcome) -> RunReport {
    match outcome {
        RunOutcome::Complete(report) => *report,
        RunOutcome::Unreachable { reason, .. } => panic!("unreachable: {reason}"),
    }
}

fn replay(name: &str, bound: Option<&str>) -> (RunReport, Vec<Request>) {
    let case = load_cases()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("the corpus has {name}"));
    let transport = transport_for(&case.scenario);
    let report = complete(replay_as(
        &case.scenario,
        Arc::new(default_handlers()),
        transport.clone(),
        Some(credential()),
        bound.map(token_host),
    ));
    (report, transport.requests())
}

fn carries_token(request: &Request) -> bool {
    request
        .headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("authorization"))
}

fn signed_urls(sent: &[Request]) -> Vec<(String, String)> {
    sent.iter()
        .filter(|r| carries_token(r))
        .map(|r| (r.method.clone(), r.url.clone()))
        .collect()
}

fn assert_only_posts_to(sent: &[Request], endpoint: &str) {
    let signed = signed_urls(sent);
    assert!(!signed.is_empty(), "nothing was signed");
    for (method, url) in &signed {
        assert_eq!((method.as_str(), url.as_str()), ("POST", endpoint));
    }
}

fn assert_kept(report: &RunReport, sent: &[Request], not_sent: &TokenNotSent) {
    assert_eq!(signed_urls(sent), Vec::<(String, String)>::new());
    assert!(!report.scorecard.vantage.credentialed);
    assert_eq!(report.token_not_sent.as_ref(), Some(not_sent));
}

#[test]
fn over_the_corpus_a_token_bound_to_a_declared_host_reaches_its_endpoint_and_nothing_else() {
    for spelled in ["mcp.example.net", "MCP.Example.NET.", "mcp.example.net:443"] {
        let (report, sent) = replay("follow-card-admit", Some(spelled));
        assert_only_posts_to(&sent, DECLARED_ENDPOINT);
        assert!(report.scorecard.vantage.credentialed, "{spelled}");
        assert_eq!(report.token_not_sent, None, "{spelled}");
    }
}

#[test]
fn over_the_corpus_a_port_names_only_that_port() {
    let (report, sent) = replay("follow-card-admit", Some("mcp.example.net:8443"));
    assert_kept(
        &report,
        &sent,
        &TokenNotSent {
            bound_host: Some("mcp.example.net:8443".to_string()),
            endpoint: Some(DECLARED_ENDPOINT.to_string()),
            followed: true,
        },
    );
}

#[test]
fn over_the_corpus_a_token_with_no_named_host_is_never_sent() {
    for (name, endpoint, followed) in [
        ("follow-card-admit", Some(DECLARED_ENDPOINT), true),
        ("run-all-pass", Some(OWN_ENDPOINT), false),
        ("api-hygiene-pass", None, false),
    ] {
        let (report, sent) = replay(name, None);
        assert_kept(
            &report,
            &sent,
            &TokenNotSent {
                bound_host: None,
                endpoint: endpoint.map(str::to_string),
                followed,
            },
        );
    }
}

#[test]
fn over_the_corpus_a_token_bound_to_another_host_never_reaches_the_audited_sites_endpoint() {
    let (report, sent) = replay("run-all-pass", Some("mcp.stripe.com"));
    assert_kept(
        &report,
        &sent,
        &TokenNotSent {
            bound_host: Some("mcp.stripe.com".to_string()),
            endpoint: Some(OWN_ENDPOINT.to_string()),
            followed: false,
        },
    );
}

#[test]
fn over_the_corpus_a_token_naming_the_audited_host_reaches_its_own_endpoint() {
    for bound in ["example.com", "EXAMPLE.com.", "example.com:443"] {
        let (report, sent) = replay("run-all-pass", Some(bound));
        assert_only_posts_to(&sent, OWN_ENDPOINT);
        assert!(report.scorecard.vantage.credentialed, "{bound}");
        assert_eq!(report.token_not_sent, None, "{bound}");
    }
}

#[test]
fn over_the_corpus_a_declared_host_gets_the_token_only_after_it_confirms_the_endpoint() {
    let (report, sent) = replay("follow-reciprocity-refused", Some("mcp.example.net"));
    assert_kept(
        &report,
        &sent,
        &TokenNotSent {
            bound_host: Some("mcp.example.net".to_string()),
            endpoint: None,
            followed: false,
        },
    );

    let (_, sent) = replay("follow-card-admit", Some("mcp.example.net"));
    let first_signed = sent
        .iter()
        .position(carries_token)
        .expect("the confirmed endpoint was signed in to");
    let confirmed = sent
        .iter()
        .position(|r| r.url == "https://mcp.example.net/mcp/server-card")
        .expect("the card that confirms the endpoint was read");
    assert!(
        confirmed < first_signed,
        "the token went out before the host confirmed the endpoint"
    );
}

#[test]
fn over_the_corpus_a_site_with_no_endpoint_keeps_a_token_naming_its_host() {
    let (report, sent) = replay("api-hygiene-pass", Some("example.com"));
    assert_kept(
        &report,
        &sent,
        &TokenNotSent {
            bound_host: Some("example.com".to_string()),
            endpoint: None,
            followed: false,
        },
    );
}

/// `common_paths` only builds candidates on the audited origin, so a root path
/// on a declared host is reached through the declaration, not through them.
#[test]
fn a_declared_endpoint_at_a_hosts_root_is_reached_through_the_declaration() {
    const ROOT_ENDPOINT: &str = "https://mcp.a.example/";
    let card = json!({
        "$schema": CARD_SCHEMA,
        "name": "com.example/fixture",
        "version": "1.0.0",
        "description": "A fixture MCP server",
        "remotes": [{ "type": "streamable-http", "url": ROOT_ENDPOINT }],
    })
    .to_string();
    let site_card =
        json!({ "name": "fixture", "version": "1", "mcp_endpoint": ROOT_ENDPOINT }).to_string();
    let rules = vec![
        MockRule::new("GET", "https://a.example/").response(MockResponse::ok(
            200,
            &[("content-type", "text/html")],
            html_body().as_bytes(),
        )),
        MockRule::new("GET", "https://a.example/.well-known/mcp/server-card.json").response(
            MockResponse::ok(200, &[("content-type", "application/json")], site_card.as_bytes()),
        ),
        MockRule::new("GET", "https://mcp.a.example/server-card").response(MockResponse::ok(
            200,
            &[("content-type", "application/mcp-server-card+json")],
            card.as_bytes(),
        )),
        MockRule::new("POST", ROOT_ENDPOINT).response(MockResponse::ok(
            200,
            &[("content-type", "application/json")],
            br#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1.0"}}}"#,
        )),
    ];
    let transport = Arc::new(MockTransport::new(rules, MockResponse::ok(404, &[], b"")));
    let fetch = FetchHandle::new(
        transport.clone(),
        Arc::new(PublicResolver),
        AUDIT_USER_AGENT,
    )
    .with_credential(credential());
    let mut input = RunInput::new("https://a.example/", fetch, Arc::new(default_handlers()));
    input.token_host = Some(token_host("mcp.a.example"));
    let report = complete(run_web_audit(input));
    let sent = transport.requests();

    assert_eq!(
        report.scorecard.mcp_endpoint.as_deref(),
        Some(ROOT_ENDPOINT)
    );
    let common_path_probes: Vec<&Request> = sent
        .iter()
        .filter(|r| {
            r.method == "POST"
                && r.url.starts_with("https://mcp.a.example/")
                && r.url != ROOT_ENDPOINT
        })
        .collect();
    assert!(
        common_path_probes.is_empty(),
        "a POST left the declared endpoint: {common_path_probes:#?}"
    );
    assert_only_posts_to(&sent, ROOT_ENDPOINT);
    let first_signed = sent.iter().position(carries_token).expect("a signed POST");
    let confirmed = sent
        .iter()
        .position(|r| r.url == "https://mcp.a.example/server-card")
        .expect("the card was read");
    assert!(confirmed < first_signed);
    assert!(report.scorecard.vantage.credentialed);
    assert_eq!(report.token_not_sent, None);
}

fn initialize_result() -> RawResponse {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "fixture", "version": "1.0" },
        },
    })
    .to_string();
    RawResponse::new(
        200,
        &[("content-type", "application/json")],
        body.as_bytes(),
    )
}

fn redirect_to(location: &str) -> RawResponse {
    RawResponse::new(307, &[("location", location), ("content-length", "0")], b"")
}

fn request_path(req: &RawRequest) -> &str {
    req.target.split('?').next().unwrap_or("/")
}

/// A site whose MCP endpoint, found on a common path, serves any client.
fn own_endpoint_site() -> Server {
    common::spawn_https(
        |req: &RawRequest| match (req.method.as_str(), request_path(req)) {
            ("GET", "/") => ok("text/html; charset=utf-8", &html_body()),
            ("POST", "/mcp") => initialize_result(),
            _ => not_found(),
        },
    )
}

/// A site whose server card names `endpoint` off its origin, and which, when
/// `own_endpoint`, serves an MCP endpoint of its own on a common path.
fn site_declaring(endpoint: String, own_endpoint: bool) -> Server {
    common::spawn_https(
        move |req: &RawRequest| match (req.method.as_str(), request_path(req)) {
            ("GET", "/") => ok("text/html; charset=utf-8", &html_body()),
            ("GET", "/.well-known/mcp/server-card.json") => ok(
                "application/json",
                &json!({ "name": "fixture", "version": "1", "mcp_endpoint": endpoint }).to_string(),
            ),
            ("POST", "/mcp") if own_endpoint => initialize_result(),
            _ => not_found(),
        },
    )
}

/// The SEP-2127 card a host serves to confirm the endpoint at `path` on itself.
fn confirming_card(req: &RawRequest, path: &str) -> RawResponse {
    let host = req.header("host").unwrap_or("localhost");
    ok(
        "application/mcp-server-card+json",
        &json!({
            "$schema": CARD_SCHEMA,
            "name": "com.example/fixture",
            "version": "1.0.0",
            "description": "A fixture MCP server",
            "remotes": [{ "type": "streamable-http", "url": format!("https://{host}{path}") }],
        })
        .to_string(),
    )
}

/// A host whose MCP endpoint at `/mcp` serves any client, and whose card there
/// names that endpoint when `confirms`.
fn declared_host(confirms: bool) -> Server {
    common::spawn_https(
        move |req: &RawRequest| match (req.method.as_str(), request_path(req)) {
            ("GET", "/mcp/server-card") if confirms => confirming_card(req, "/mcp"),
            ("POST", "/mcp") => initialize_result(),
            _ => not_found(),
        },
    )
}

/// `name:port` for a server, so one listening on `127.0.0.1` can read as
/// another host than a `127.0.0.1` target.
fn url_host(name: &str, server: &Server) -> String {
    format!("{name}:{}", server.addr.port())
}

fn endpoint_at(name: &str, server: &Server) -> String {
    format!("https://{}/mcp", url_host(name, server))
}

fn signed(server: &Server) -> Vec<RawRequest> {
    server
        .hits()
        .into_iter()
        .filter(|r| r.header("authorization").is_some())
        .collect()
}

fn assert_signed_only_at(server: &Server, path: &str) {
    let signed = signed(server);
    assert!(
        !signed.is_empty(),
        "nothing was signed: {:#?}",
        server.hits()
    );
    let bearer = format!("Bearer {TOKEN}");
    for request in &signed {
        assert_eq!(
            (
                request.method.as_str(),
                request_path(request),
                request.header("authorization")
            ),
            ("POST", path, Some(bearer.as_str())),
        );
    }
}

fn assert_never_signed(server: &Server, what: &str) {
    let signed = signed(server);
    assert!(signed.is_empty(), "{what} received the token: {signed:#?}");
}

/// The first signed request came after the host's card was read.
fn assert_signed_after_card(server: &Server, card_path: &str) {
    let hits = server.hits();
    let first_signed = hits
        .iter()
        .position(|r| r.header("authorization").is_some())
        .expect("a signed request");
    let confirmed = hits
        .iter()
        .position(|r| request_path(r) == card_path)
        .expect("the card was read");
    assert!(
        confirmed < first_signed,
        "the token went out before the host confirmed the endpoint: {hits:#?}"
    );
}

/// `anc web <target> --output json` with the token in `ANC_WEB_TOKEN`,
/// `ANC_WEB_TOKEN_HOST` set to `variable`, and `args` after the target.
fn run_with_env_token(target: &str, variable: Option<&str>, args: &[&str]) -> Output {
    let mut cmd = anc();
    cmd.args(["web", target, "--output", "json"])
        .args(args)
        .env("ANC_WEB_TOKEN", TOKEN);
    if let Some(variable) = variable {
        cmd.env("ANC_WEB_TOKEN_HOST", variable);
    }
    cmd.assert().get_output().clone()
}

/// `anc web <target> --output json --token-stdin` with the token on stdin, and
/// `args` after it.
fn run_with_stdin_token(target: &str, args: &[&str]) -> Output {
    anc()
        .args(["web", target, "--output", "json", "--token-stdin"])
        .args(args)
        .write_stdin(format!("{TOKEN}\n"))
        .assert()
        .get_output()
        .clone()
}

fn scorecard(output: &Output) -> Value {
    serde_json::from_str(&stdout_of(output)).unwrap_or_else(|e| {
        panic!(
            "stdout is not a JSON scorecard: {e}\nstderr: {}",
            stderr_of(output)
        )
    })
}

fn assert_presented(output: &Output, presented: bool) {
    assert_eq!(
        scorecard(output)["vantage"]["credentialed"],
        json!(presented)
    );
    let stderr = stderr_of(output);
    assert_eq!(stderr.contains(NOT_SENT), !presented, "{stderr}");
    for (stream, text) in [("stdout", stdout_of(output)), ("stderr", stderr)] {
        assert!(!text.contains(TOKEN), "{stream} printed the token:\n{text}");
    }
}

#[test]
fn a_token_bound_to_another_host_never_reaches_the_audited_site() {
    let server = own_endpoint_site();
    let output = run_with_env_token(&host_of(&server), Some("mcp.stripe.com"), &[]);
    assert_never_signed(&server, "the audited site");
    assert_presented(&output, false);
    let stderr = stderr_of(&output);
    assert!(stderr.contains("mcp.stripe.com"), "{stderr}");

    let server = own_endpoint_site();
    let output = run_with_stdin_token(&host_of(&server), &["--token-host", "mcp.stripe.com"]);
    assert_never_signed(&server, "the audited site");
    assert_presented(&output, false);
    let stderr = stderr_of(&output);
    assert!(stderr.contains("mcp.stripe.com"), "{stderr}");
}

/// The audited site's own endpoint outranks a declared one, so a token bound
/// to the declared host has no endpoint of record there. Neither host gets it,
/// and the warning names that case.
#[test]
fn a_token_bound_to_a_declared_host_never_reaches_a_site_with_its_own_endpoint() {
    let declared = declared_host(true);
    let site = site_declaring(endpoint_at("localhost", &declared), true);
    let bound = url_host("localhost", &declared);
    let output = run_with_env_token(&host_of(&site), Some(&bound), &[]);
    assert_eq!(
        scorecard(&output)["mcp_endpoint"],
        json!(site.url("/mcp")),
        "the site's own endpoint is the endpoint of record"
    );
    assert_never_signed(&site, "the audited site");
    assert_never_signed(&declared, "the declared host");
    assert_presented(&output, false);
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("the audited site's own MCP endpoint"),
        "{stderr}"
    );
    assert!(stderr.contains(&site.url("/mcp")), "{stderr}");
}

#[test]
fn a_token_with_no_named_host_is_never_sent() {
    let own = own_endpoint_site();
    let output = run_with_env_token(&host_of(&own), None, &[]);
    assert_never_signed(&own, "the audited site");
    assert_presented(&output, false);
    let stderr = stderr_of(&output);
    assert!(stderr.contains("ANC_WEB_TOKEN_HOST"), "{stderr}");

    let declared = declared_host(true);
    let endpoint = endpoint_at("localhost", &declared);
    let site = site_declaring(endpoint.clone(), false);
    let output = run_with_env_token(&host_of(&site), None, &[]);
    assert_eq!(scorecard(&output)["mcp_endpoint"], json!(endpoint));
    assert_never_signed(&site, "the audited site");
    assert_never_signed(&declared, "the declared host");
    assert_presented(&output, false);

    // A variable set to nothing names no host.
    let empty = own_endpoint_site();
    let output = run_with_env_token(&host_of(&empty), Some(""), &[]);
    assert_never_signed(&empty, "the audited site");
    assert_presented(&output, false);

    let on_stdin = own_endpoint_site();
    let output = run_with_stdin_token(&host_of(&on_stdin), &[]);
    assert_never_signed(&on_stdin, "the audited site");
    assert_presented(&output, false);
    let stderr = stderr_of(&output);
    assert!(stderr.contains("--token-host"), "{stderr}");
}

/// An audited site chooses the `--token-host` value anc.dev prints, so that
/// flag cannot be what releases a token exported for something else.
#[test]
fn an_exported_token_is_not_bound_by_the_flag_alone() {
    let own = own_endpoint_site();
    let host = host_of(&own);
    let output = run_with_env_token(&host, None, &["--token-host", &host]);
    assert_never_signed(&own, "the audited site");
    assert_presented(&output, false);
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("--token-host alone does not bind it"),
        "{stderr}"
    );

    let declared = declared_host(true);
    let site = site_declaring(endpoint_at("localhost", &declared), false);
    let suggested = url_host("localhost", &declared);
    let output = run_with_env_token(&host_of(&site), None, &["--token-host", &suggested]);
    assert_never_signed(&site, "the audited site");
    assert_never_signed(&declared, "the host the flag named");
    assert_presented(&output, false);
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("--token-host alone does not bind it"),
        "{stderr}"
    );
}

#[test]
fn an_exported_token_goes_where_its_exported_host_says() {
    let bare = own_endpoint_site();
    let host = host_of(&bare);
    let output = run_with_env_token(&host, Some(&host), &[]);
    assert_signed_only_at(&bare, "/mcp");
    assert_presented(&output, true);

    let matching = own_endpoint_site();
    let host = host_of(&matching);
    let output = run_with_env_token(&host, Some(&host), &["--token-host", &host]);
    assert_signed_only_at(&matching, "/mcp");
    assert_presented(&output, true);
}

#[test]
fn a_flag_that_disagrees_with_the_exported_host_sends_nothing() {
    let named_by_the_flag = own_endpoint_site();
    let host = host_of(&named_by_the_flag);
    let output = run_with_env_token(&host, Some("mcp.stripe.com"), &["--token-host", &host]);
    assert_never_signed(&named_by_the_flag, "the host the flag named");
    assert_presented(&output, false);

    let named_by_the_variable = own_endpoint_site();
    let host = host_of(&named_by_the_variable);
    let output = run_with_env_token(&host, Some(&host), &["--token-host", "mcp.stripe.com"]);
    assert_never_signed(&named_by_the_variable, "the host the variable named");
    assert_presented(&output, false);
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains(&format!("ANC_WEB_TOKEN_HOST names {host}")),
        "{stderr}"
    );
    assert!(
        stderr.contains("--token-host names mcp.stripe.com"),
        "{stderr}"
    );
}

#[test]
fn a_token_on_stdin_is_bound_by_the_flag_and_outranks_the_variable() {
    let own = own_endpoint_site();
    let host = host_of(&own);
    let output = run_with_stdin_token(&host, &["--token-host", &host]);
    assert_signed_only_at(&own, "/mcp");
    assert_presented(&output, true);

    let beside_exported = own_endpoint_site();
    let host = host_of(&beside_exported);
    let output = anc()
        .args(["web", &host, "--output", "json", "--token-stdin"])
        .args(["--token-host", &host])
        .env("ANC_WEB_TOKEN", "the-exported-token")
        .env("ANC_WEB_TOKEN_HOST", "mcp.stripe.com")
        .write_stdin(format!("{TOKEN}\n"))
        .assert()
        .get_output()
        .clone();
    assert_signed_only_at(&beside_exported, "/mcp");
    assert_presented(&output, true);
}

/// The variable is the other channel, whatever it holds.
#[test]
fn the_exported_host_does_not_bind_a_token_on_stdin() {
    for variable in ["SELF", "https://not-a-host.example"] {
        let own = own_endpoint_site();
        let host = host_of(&own);
        let value = if variable == "SELF" {
            host.as_str()
        } else {
            variable
        };
        let output = anc()
            .args(["web", &host, "--output", "json", "--token-stdin"])
            .env("ANC_WEB_TOKEN_HOST", value)
            .write_stdin(format!("{TOKEN}\n"))
            .assert()
            .get_output()
            .clone();
        assert_never_signed(&own, "the audited site");
        assert_presented(&output, false);
        let stderr = stderr_of(&output);
        assert!(
            stderr.contains("ANC_WEB_TOKEN_HOST binds only ANC_WEB_TOKEN"),
            "{stderr}"
        );
    }
}

#[test]
fn a_token_naming_the_audited_host_reaches_its_own_endpoint() {
    let own = own_endpoint_site();
    let host = host_of(&own);
    let output = run_with_env_token(&host, Some(&host), &[]);
    assert_signed_only_at(&own, "/mcp");
    assert_presented(&output, true);
}

#[test]
fn a_token_bound_to_a_declared_host_reaches_it_after_reciprocity_and_nowhere_else() {
    enum Named {
        Variable(&'static str),
        VariableAndFlag(&'static str, &'static str),
        StdinFlag(&'static str),
    }
    for named in [
        Named::Variable("localhost"),
        Named::Variable("LOCALHOST."),
        Named::VariableAndFlag("localhost", "LOCALHOST."),
        Named::StdinFlag("localhost"),
        Named::StdinFlag("LOCALHOST."),
    ] {
        let declared = declared_host(true);
        let site = site_declaring(endpoint_at("localhost", &declared), false);
        let target = host_of(&site);
        let output = match named {
            Named::Variable(name) => {
                run_with_env_token(&target, Some(&url_host(name, &declared)), &[])
            }
            Named::VariableAndFlag(name, flag) => run_with_env_token(
                &target,
                Some(&url_host(name, &declared)),
                &["--token-host", &url_host(flag, &declared)],
            ),
            Named::StdinFlag(name) => {
                run_with_stdin_token(&target, &["--token-host", &url_host(name, &declared)])
            }
        };
        assert_signed_only_at(&declared, "/mcp");
        assert_never_signed(&site, "the audited site");
        assert_presented(&output, true);
        assert_signed_after_card(&declared, "/mcp/server-card");
    }
}

#[test]
fn a_declared_host_that_confirms_nothing_never_gets_the_token() {
    let declared = declared_host(false);
    let site = site_declaring(endpoint_at("localhost", &declared), false);
    let bound = url_host("localhost", &declared);
    let output = run_with_env_token(&host_of(&site), Some(&bound), &[]);
    assert_never_signed(&declared, "the unconfirmed host");
    assert!(
        declared.hits().iter().all(|r| r.method != "POST"),
        "the unconfirmed host received a wire probe"
    );
    assert_presented(&output, false);
}

/// A declared endpoint at a host's root over HTTPS: reached through the
/// declaration, signed after its own card names it.
#[test]
fn a_token_reaches_a_declared_endpoint_at_a_hosts_root() {
    let declared =
        common::spawn_https(
            |req: &RawRequest| match (req.method.as_str(), request_path(req)) {
                ("GET", "/server-card") => confirming_card(req, "/"),
                ("POST", "/") => initialize_result(),
                _ => not_found(),
            },
        );
    let bound = url_host("localhost", &declared);
    let endpoint = format!("https://{bound}/");
    let site = site_declaring(endpoint.clone(), false);
    let output = run_with_env_token(&host_of(&site), Some(&bound), &[]);
    assert_eq!(scorecard(&output)["mcp_endpoint"], json!(endpoint));
    assert_signed_only_at(&declared, "/");
    assert_signed_after_card(&declared, "/server-card");
    assert_never_signed(&site, "the audited site");
    assert_presented(&output, true);
}

/// A host with no port means port 443, so it names no ephemeral port, and
/// naming the target's own host and port leaves a declared port on it out.
#[test]
fn a_port_is_part_of_the_host_a_token_is_bound_to() {
    let exact = declared_host(true);
    let site = site_declaring(endpoint_at("localhost", &exact), false);
    let bound = url_host("localhost", &exact);
    let output = run_with_env_token(&url_host("localhost", &site), Some(&bound), &[]);
    assert_signed_only_at(&exact, "/mcp");
    assert_signed_after_card(&exact, "/mcp/server-card");
    assert_presented(&output, true);

    for bound in ["localhost", "localhost:443", "localhost:1"] {
        let other_port = declared_host(true);
        let site = site_declaring(endpoint_at("localhost", &other_port), false);
        let output = run_with_env_token(&url_host("localhost", &site), Some(bound), &[]);
        assert_eq!(
            scorecard(&output)["mcp_endpoint"],
            json!(endpoint_at("localhost", &other_port)),
            "the endpoint of record is on the declared port"
        );
        assert_never_signed(&other_port, "a port the host did not name");
        assert_presented(&output, false);
    }

    let declared = declared_host(true);
    let site = site_declaring(endpoint_at("localhost", &declared), false);
    let target = url_host("localhost", &site);
    let output = run_with_env_token(&target, Some(&target), &[]);
    assert_eq!(
        scorecard(&output)["mcp_endpoint"],
        json!(endpoint_at("localhost", &declared)),
        "the endpoint of record is on the declared port"
    );
    assert_never_signed(&declared, "another port on the target's host name");
    assert_never_signed(&site, "a site with no endpoint");
    assert_presented(&output, false);
}

#[test]
fn a_token_on_stdin_follows_the_same_rule() {
    let unnamed = own_endpoint_site();
    let output = run_with_stdin_token(&host_of(&unnamed), &[]);
    assert_never_signed(&unnamed, "the audited site");
    assert_presented(&output, false);

    let elsewhere = own_endpoint_site();
    let output = run_with_stdin_token(&host_of(&elsewhere), &["--token-host", "mcp.stripe.com"]);
    assert_never_signed(&elsewhere, "the audited site");
    assert_presented(&output, false);

    let unbound = declared_host(true);
    let site = site_declaring(endpoint_at("localhost", &unbound), false);
    let output = run_with_stdin_token(&host_of(&site), &[]);
    assert_never_signed(&unbound, "a declared host");
    assert_presented(&output, false);

    let bound = declared_host(true);
    let site = site_declaring(endpoint_at("localhost", &bound), false);
    let output = run_with_stdin_token(
        &host_of(&site),
        &["--token-host", &url_host("localhost", &bound)],
    );
    assert_signed_only_at(&bound, "/mcp");
    assert_signed_after_card(&bound, "/mcp/server-card");
    assert_never_signed(&site, "the audited site");
    assert_presented(&output, true);
}

#[test]
fn a_bare_target_binds_a_token_on_stdin_and_still_leaves_the_variable_unread() {
    let server = own_endpoint_site();
    let output = anc()
        .args([&host_of(&server), "--output", "json", "--token-stdin"])
        .args(["--token-host", "mcp.stripe.com"])
        .write_stdin(format!("{TOKEN}\n"))
        .assert()
        .get_output()
        .clone();
    assert_never_signed(&server, "the audited site");
    assert_presented(&output, false);

    let server = own_endpoint_site();
    let host = host_of(&server);
    let output = anc()
        .args([&host, "--output", "json", "--token-stdin"])
        .args(["--token-host", &host])
        .write_stdin(format!("{TOKEN}\n"))
        .assert()
        .get_output()
        .clone();
    assert_signed_only_at(&server, "/mcp");
    assert_presented(&output, true);

    // Bound to this very host, an exported token on a bare target stays unread,
    // so there is none to keep.
    let server = own_endpoint_site();
    let host = host_of(&server);
    let output = anc()
        .args([&host, "--output", "json"])
        .env("ANC_WEB_TOKEN", TOKEN)
        .env("ANC_WEB_TOKEN_HOST", &host)
        .assert()
        .get_output()
        .clone();
    assert_never_signed(&server, "a bare target");
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("ANC_WEB_TOKEN is set and not read"),
        "{stderr}"
    );
    assert!(
        stderr.contains("with ANC_WEB_TOKEN_HOST set sends it"),
        "the notice names what would send it: {stderr}"
    );
    assert!(!stderr.contains(NOT_SENT), "{stderr}");
    assert_eq!(scorecard(&output)["vantage"]["credentialed"], json!(false));
}

#[test]
fn a_kept_token_warns_and_leaves_stdout_the_scorecard_a_tokenless_run_prints() {
    let server = own_endpoint_site();
    let host = host_of(&server);
    let tokenless = anc()
        .args(["web", &host, "--output", "json"])
        .assert()
        .get_output()
        .clone();
    let kept = run_with_env_token(&host, Some("mcp.stripe.com"), &[]);
    assert_eq!(stdout_of(&kept), stdout_of(&tokenless));
    assert_eq!(kept.status.code(), tokenless.status.code());
    assert!(serde_json::from_str::<Value>(&stdout_of(&kept)).is_ok());
    assert!(!stdout_of(&kept).contains("token-not-sent"));
    let warnings: Vec<String> = stderr_of(&kept)
        .lines()
        .filter(|line| line.starts_with(NOT_SENT))
        .map(str::to_string)
        .collect();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("mcp.stripe.com"), "{warnings:?}");
    assert!(warnings[0].contains(&server.url("/mcp")), "{warnings:?}");

    // `-q` drops progress, not a warning about where the token went.
    let quiet = run_with_env_token(&host, Some("mcp.stripe.com"), &["-q"]);
    assert!(
        stderr_of(&quiet).contains(NOT_SENT),
        "{}",
        stderr_of(&quiet)
    );

    let text = anc()
        .args(["web", &host])
        .env("ANC_WEB_TOKEN", TOKEN)
        .env("ANC_WEB_TOKEN_HOST", "mcp.stripe.com")
        .assert()
        .get_output()
        .clone();
    assert!(stderr_of(&text).contains(NOT_SENT), "{}", stderr_of(&text));
    assert!(!stdout_of(&text).contains(NOT_SENT), "{}", stdout_of(&text));
}

/// `credentialed` says what went out, not what the run held.
#[test]
fn credentialed_reads_true_only_when_a_request_carried_the_token() {
    let unbound = common::web_cli::fixture_site();
    let output = run_with_env_token(&host_of(&unbound), None, &[]);
    assert_never_signed(&unbound, "a site with no endpoint");
    assert_presented(&output, false);

    let bound = common::web_cli::fixture_site();
    let host = host_of(&bound);
    let output = run_with_env_token(&host, Some(&host), &[]);
    assert_never_signed(&bound, "a site with no endpoint");
    assert_presented(&output, false);

    let served = own_endpoint_site();
    let host = host_of(&served);
    let output = run_with_env_token(&host, Some(&host), &[]);
    assert_signed_only_at(&served, "/mcp");
    assert_presented(&output, true);
}

#[test]
fn a_malformed_token_host_ends_the_run_before_any_request() {
    let server = own_endpoint_site();
    let host = host_of(&server);
    for bad in [
        "https://mcp.stripe.com",
        "mcp.stripe.com/mcp",
        "mcp.stripe.com/",
        "user@mcp.stripe.com",
        "mcp.stripe.com?x=1",
        "mcp.stripe.com#x",
        "mcp.stripe.com:",
        "mcp.stripe.com:notaport",
        "mcp.stripe.com:99999",
        "*.stripe.com",
        "two words",
        "[not-an-address]",
        "mcp..stripe.com",
        "",
        ".",
    ] {
        let assert = anc()
            .args(["web", &host, "--output", "json", "--token-host", bad])
            .env("ANC_WEB_TOKEN", TOKEN)
            .assert()
            .code(2);
        let output = assert.get_output();
        assert_eq!(stdout_of(output), "", "{bad}");
        let envelope: Value =
            serde_json::from_str(stderr_of(output).trim()).expect("a JSON error envelope");
        assert_eq!(envelope["error"], "invalid-token-host", "{bad}");
        assert_eq!(envelope["kind"], "usage", "{bad}");
        assert_eq!(envelope["exit_code"], 2, "{bad}");
        assert_eq!(envelope["argument"], "--token-host <HOST>", "{bad}");
        assert_eq!(envelope["value"], bad.replace("user@", ""), "{bad}");
        assert_eq!(envelope["next_step"]["action"], "show-help", "{bad}");
        assert_eq!(envelope["next_step"]["command"], "anc web --help", "{bad}");
        assert!(envelope["next_step"]["docs"].is_string(), "{bad}");
        assert!(envelope["message"].is_string(), "{bad}");
    }

    for with_password in ["https://user:secret@x.dev/", "user:secret@x.dev"] {
        let text = anc()
            .args(["web", &host, "--token-host", with_password])
            .assert()
            .code(2);
        let stderr = stderr_of(text.get_output());
        assert!(stderr.starts_with("error: "), "{stderr}");
        assert!(stderr.contains("next: anc web --help"), "{stderr}");
        assert!(!stderr.contains("secret"), "a password printed: {stderr}");
        let json = anc()
            .args(["web", &host, "--output", "json"])
            .args(["--token-host", with_password])
            .assert()
            .code(2);
        let stderr = stderr_of(json.get_output());
        assert!(!stderr.contains("secret"), "a password printed: {stderr}");
    }

    assert!(
        server.hits().is_empty(),
        "a malformed host still sent requests"
    );
}

#[test]
fn the_variable_only_matters_to_a_run_holding_a_token() {
    let server = own_endpoint_site();
    let host = host_of(&server);
    let tokenless = anc()
        .args(["web", &host, "--output", "json"])
        .assert()
        .get_output()
        .clone();

    for value in ["https://mcp.stripe.com", "mcp.stripe.com", ""] {
        let output = anc()
            .args(["web", &host, "--output", "json"])
            .env("ANC_WEB_TOKEN_HOST", value)
            .assert()
            .get_output()
            .clone();
        assert_eq!(output.status.code(), tokenless.status.code(), "{value:?}");
        assert_eq!(stdout_of(&output), stdout_of(&tokenless), "{value:?}");
        assert!(!stderr_of(&output).contains("TOKEN_HOST"), "{value:?}");
        assert!(!stderr_of(&output).contains(NOT_SENT), "{value:?}");
    }
    assert_never_signed(&server, "a run with no token");

    let before = server.hits().len();
    let refused = anc()
        .args(["web", &host, "--output", "json"])
        .env("ANC_WEB_TOKEN", TOKEN)
        .env("ANC_WEB_TOKEN_HOST", "https://mcp.stripe.com")
        .assert()
        .code(2);
    let envelope: Value = serde_json::from_str(stderr_of(refused.get_output()).trim())
        .expect("a JSON error envelope");
    assert_eq!(envelope["error"], "invalid-token-host");
    assert_eq!(envelope["argument"], "ANC_WEB_TOKEN_HOST");
    assert_eq!(envelope["value"], "https://mcp.stripe.com");
    assert_eq!(
        server.hits().len(),
        before,
        "a malformed host sent requests"
    );
}

/// An audited endpoint that answers a signed POST with a 307 to `to`, and
/// serves a client without a token.
fn endpoint_redirecting_signed_posts(to: String) -> Server {
    common::spawn_https(
        move |req: &RawRequest| match (req.method.as_str(), request_path(req)) {
            ("GET", "/") => ok("text/html; charset=utf-8", &html_body()),
            ("POST", "/mcp") if req.header("authorization").is_some() => redirect_to(&to),
            ("POST", "/mcp") => initialize_result(),
            _ => not_found(),
        },
    )
}

#[test]
fn a_redirect_never_carries_the_token_off_its_bound_host() {
    // To another host, and to another port on the same host name.
    for name in ["localhost", "127.0.0.1"] {
        let elsewhere = declared_host(true);
        let endpoint = endpoint_redirecting_signed_posts(endpoint_at(name, &elsewhere));
        let bound = host_of(&endpoint);
        let output = run_with_env_token(&bound, Some(&bound), &[]);
        assert_signed_only_at(&endpoint, "/mcp");
        assert_never_signed(&elsewhere, "where the endpoint redirected");
        assert_presented(&output, true);
    }

    // A confirmed endpoint on a declared host takes no redirect at all.
    let elsewhere = declared_host(true);
    let target = endpoint_at("127.0.0.1", &elsewhere);
    let redirecting = common::spawn_https(move |req: &RawRequest| {
        match (req.method.as_str(), request_path(req)) {
            ("GET", "/mcp/server-card") => confirming_card(req, "/mcp"),
            ("POST", "/mcp") => redirect_to(&target),
            _ => not_found(),
        }
    });
    let site = site_declaring(endpoint_at("localhost", &redirecting), false);
    let bound = url_host("localhost", &redirecting);
    let output = run_with_env_token(&host_of(&site), Some(&bound), &[]);
    assert_signed_only_at(&redirecting, "/mcp");
    assert_never_signed(&elsewhere, "where the confirmed endpoint redirected");
    assert_presented(&output, true);
}

#[test]
fn help_states_where_a_token_is_bound() {
    let assert = anc().args(["web", "--help"]).assert().code(0);
    let help = stdout_of(assert.get_output());
    for needle in [
        "--token-host <HOST>",
        "ANC_WEB_TOKEN_HOST",
        "token-not-sent",
        "export ANC_WEB_TOKEN=<token> ANC_WEB_TOKEN_HOST=<host>",
        "anc web mcp.example.com --token-host mcp.example.com --token-stdin",
    ] {
        assert!(help.contains(needle), "`anc web --help` lacks {needle:?}");
    }
}
