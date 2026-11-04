//! Each check's edges, held to what its code reads: a resolver or handler that
//! reaches for a source its edges do not name reads it unsettled, and one that
//! names a source it never reads waits for nothing. Rust has no way to watch a
//! field being read, so each read is found by its effect: a signal present in
//! every context and removed one at a time, where a resolver or handler that
//! reads the signal answers differently.

#![cfg(feature = "web")]

mod common;

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use agentnative::web_audit::engine::antecedents::{
    AntecedentContext, Resolution, prerequisites, resolve,
};
use agentnative::web_audit::engine::prerequisites::{Prerequisite, row_prerequisites};
use agentnative::web_audit::engine::sign_in::{McpAuthRequired, is_wire_probe};
use agentnative::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus};
use agentnative::web_audit::fetch::ProbeResponse;
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::handlers::mcp::ops::Era;
use agentnative::web_audit::headers::Headers;
use agentnative::web_audit::mock::{MockResponse, MockTransport};
use agentnative::web_audit::registry::{AntecedentToken, CHECKS, WebCheck};
use agentnative::web_audit::scorecard::DeclaredSiteType;
use common::handler_context::{BASE, context};
use serde_json::{Value, json};

const ENDPOINT: &str = "https://example.com/mcp";
const SOURCES: [&str; 7] = [
    "robots",
    "llms-txt",
    "llms-full-txt",
    "openapi",
    "oauth-discovery",
    "sitemap",
    "accept-markdown",
];
const WIRE: [&str; 2] = ["mcp-initialize", "mcp-server-discover"];

fn source(id: &str) -> ProbeOutcome {
    let body = match id {
        "llms-txt" => "- [API](/api/v1/openapi.json)\n",
        "sitemap" => "<loc>https://example.com/openapi.json</loc>",
        _ => "x",
    };
    let evidence = json!({
        "status": 200,
        "ok": true,
        "body": body,
        "capabilities": ["resources"],
    });
    ProbeOutcome::new(
        ProbeStatus::Pass,
        vec![evidence.as_object().cloned().unwrap_or_default()],
    )
}

fn challenged() -> ProbeOutcome {
    let evidence = json!({ "status": 401, "www_authenticate": "Bearer" });
    ProbeOutcome::new(
        ProbeStatus::Absent,
        vec![evidence.as_object().cloned().unwrap_or_default()],
    )
}

fn html_root() -> ProbeResponse {
    ProbeResponse {
        status: Some(200),
        headers: Headers::from_pairs([("content-type", "text/html")]),
        body: "<html></html>".to_string(),
        error: None,
        elapsed_ms: 0,
        truncated: false,
        refused: None,
    }
}

fn auth() -> McpAuthRequired {
    McpAuthRequired {
        endpoint: ENDPOINT.to_string(),
        challenge: None,
        lane: Era::Legacy,
        metadata_url: "https://example.com/.well-known/oauth-protected-resource/mcp".to_string(),
        metadata: Default::default(),
    }
}

/// What resolving `token` turns on, across the contexts it can see: a source
/// whose presence moves the resolution in any of them.
fn token_reads(token: AntecedentToken) -> BTreeSet<&'static str> {
    let root = html_root();
    let auth = auth();
    // A source reads differently through its pass and through the 401 it drew,
    // so each that a resolver can read a challenge from is tried both ways.
    let challenges: [&[&str]; 3] = [&[], &WIRE, &["openapi"]];
    let mut reads = BTreeSet::new();
    for site_type in [
        None,
        Some(DeclaredSiteType::Content),
        Some(DeclaredSiteType::Api),
    ] {
        for root in [None, Some(&root)] {
            for endpoint in [None, Some(ENDPOINT)] {
                for mcp_auth in [None, Some(&auth)] {
                    for challenged_sources in challenges {
                        for read in SOURCES.iter().chain(WIRE.iter()).copied() {
                            let full: HashMap<String, ProbeOutcome> = SOURCES
                                .iter()
                                .chain(WIRE.iter())
                                .map(|id| {
                                    let outcome = if challenged_sources.contains(id) {
                                        challenged()
                                    } else {
                                        source(id)
                                    };
                                    (id.to_string(), outcome)
                                })
                                .collect();
                            let only: HashMap<String, ProbeOutcome> = full
                                .iter()
                                .filter(|(id, _)| id.as_str() == read)
                                .map(|(id, o)| (id.clone(), o.clone()))
                                .collect();
                            let mut without = full.clone();
                            without.remove(read);
                            let at = |sources: &HashMap<String, ProbeOutcome>| -> Resolution {
                                resolve(
                                    token,
                                    &AntecedentContext {
                                        site_type,
                                        mcp_endpoint: endpoint,
                                        discovery_evidence: &[],
                                        root,
                                        sources,
                                        token_less: sources,
                                        api_anchors: false,
                                        follow_unmet: None,
                                        mcp_auth,
                                        mcp_lane: Some(Era::Legacy),
                                    },
                                )
                            };
                            if at(&full) != at(&without) || at(&only) != at(&HashMap::new()) {
                                reads.insert(read);
                            }
                        }
                    }
                }
            }
        }
    }
    reads
}

fn declared_sources(edges: &[Prerequisite]) -> BTreeSet<&'static str> {
    edges
        .iter()
        .filter_map(|edge| match edge {
            Prerequisite::Source(id) => Some(*id),
            _ => None,
        })
        .collect()
}

fn waits_on_mcp(edges: &[Prerequisite]) -> bool {
    edges
        .iter()
        .any(|edge| matches!(edge, Prerequisite::McpSettled | Prerequisite::McpNotified))
}

#[test]
fn each_token_declares_exactly_the_sources_its_resolver_reads() {
    let tokens: BTreeSet<&'static str> = CHECKS.iter().map(|c| c.antecedent.as_str()).collect();
    let mut seen = BTreeSet::new();
    for check in CHECKS {
        let token = check.antecedent;
        if !seen.insert(token.as_str()) {
            continue;
        }
        let edges = prerequisites(token);
        let reads = token_reads(token);
        let (wire, plain): (BTreeSet<&str>, BTreeSet<&str>) =
            reads.iter().copied().partition(|id| is_wire_probe(id));
        assert_eq!(
            declared_sources(edges),
            plain,
            "{}: declared {edges:?}",
            token.as_str()
        );
        if !wire.is_empty() {
            assert!(
                waits_on_mcp(edges),
                "{}: reads the wire probes {wire:?} without waiting on the MCP step",
                token.as_str()
            );
        }
    }
    assert_eq!(seen, tokens);
}

// A signal a row's handler can read from the context, and the edge that
// guarantees it holds what its source returned by then.
struct Signal {
    name: &'static str,
    edges: &'static [Prerequisite],
    remove: fn(&mut HandlerContext),
}

const SIGNALS: &[Signal] = &[
    Signal {
        name: "the retained llms.txt",
        edges: &[Prerequisite::Source("llms-txt")],
        remove: |ctx| {
            let mut bodies = (*ctx.retained_bodies).clone();
            bodies.remove("llms-txt");
            ctx.retained_bodies = Arc::new(bodies);
        },
    },
    Signal {
        name: "the retained OpenAPI description",
        edges: &[Prerequisite::Source("openapi")],
        remove: |ctx| {
            let mut bodies = (*ctx.retained_bodies).clone();
            bodies.remove("openapi");
            ctx.retained_bodies = Arc::new(bodies);
        },
    },
    Signal {
        name: "the scoped directories",
        edges: &[
            Prerequisite::Source("llms-txt"),
            Prerequisite::Source("sitemap"),
        ],
        remove: |ctx| ctx.scoped_dirs = Arc::new(Vec::new()),
    },
    Signal {
        name: "the declared descriptions",
        edges: &[Prerequisite::Source("openapi")],
        remove: |ctx| ctx.api_description_bodies = Arc::new(HashMap::new()),
    },
    Signal {
        name: "the session",
        edges: &[Prerequisite::McpNotified],
        remove: |ctx| ctx.mcp_session_id = None,
    },
    Signal {
        name: "the sign-in reading",
        edges: &[Prerequisite::McpSettled],
        remove: |ctx| ctx.mcp_auth = None,
    },
];

/// An outcome and the requests behind it, as text a comparison can read.
fn observe(check: &WebCheck, ctx: &HandlerContext, transport: &MockTransport) -> String {
    let before = transport.requests().len();
    let outcome = match default_handlers().dispatch(check, ctx) {
        agentnative::web_audit::engine::Dispatch::Outcome(outcome) => outcome,
        agentnative::web_audit::engine::Dispatch::Unported(kind) => panic!("{kind}: not ported"),
    };
    let mut evidence: Vec<Value> = outcome
        .evidence
        .iter()
        .map(|row| {
            let mut row = row.clone();
            row.remove("elapsed_ms");
            Value::Object(row)
        })
        .collect();
    evidence.sort_by_key(|row| row.to_string());
    let requests: Vec<String> = transport.requests()[before..]
        .iter()
        .map(|r| {
            let session = r
                .headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case("mcp-session-id"));
            format!("{} {} session={session}", r.method, r.url)
        })
        .collect();
    format!("{:?} {evidence:?} {requests:?}", outcome.status)
}

fn rich_context(transport: Arc<MockTransport>) -> HandlerContext {
    let mut ctx = context(transport);
    ctx.mcp_endpoint = Some(ENDPOINT.to_string());
    ctx.root = Some(Arc::new(html_root()));
    ctx.scoped_dirs = Arc::new(vec!["/docs".to_string()]);
    ctx.retained_bodies = Arc::new(HashMap::from([
        (
            "llms-txt".to_string(),
            "# Site\n\n> Summary\n\n- [Docs](https://example.com/docs/a.md)\n".to_string(),
        ),
        (
            "openapi".to_string(),
            r#"{"openapi":"3.1.0","paths":{"/v1/things":{"get":{}}}}"#.to_string(),
        ),
    ]));
    ctx.api_description_bodies = Arc::new(HashMap::from([(
        format!("{BASE}openapi.json"),
        r#"{"openapi":"3.1.0","paths":{"/v1/things":{"get":{}}}}"#.to_string(),
    )]));
    ctx.mcp_session_id = Some("sess-1".to_string());
    ctx.mcp_auth = Some(Arc::new(auth()));
    ctx
}

/// A row whose handler answers differently without a signal waits on what
/// guarantees that signal.
#[test]
fn every_check_waits_on_what_its_handler_reads_from_the_context() {
    let answer = MockResponse::ok(
        200,
        &[("content-type", "text/plain"), ("mcp-session-id", "sess-1")],
        b"# Doc\n",
    );
    let mut problems = Vec::new();
    for check in CHECKS {
        for signal in SIGNALS {
            let transport = Arc::new(MockTransport::new(Vec::new(), answer.clone()));
            let with = rich_context(transport.clone());
            let baseline = observe(check, &with, &transport);
            let mut without = rich_context(transport.clone());
            (signal.remove)(&mut without);
            if observe(check, &without, &transport) == baseline {
                continue;
            }
            let edges = row_prerequisites(check);
            let covered = signal.edges.iter().all(|edge| {
                edges.contains(edge)
                    || (*edge == Prerequisite::McpSettled
                        && edges.contains(&Prerequisite::McpNotified))
            });
            if !covered {
                problems.push(format!(
                    "{} reads {} and waits only on {edges:?}",
                    check.id, signal.name
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
