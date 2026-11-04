//! A check starts when what it reads has settled, not when the slowest source
//! has: one stalled source holds one slot and the checks that read it, and every
//! other check runs on. The handlers are stand-ins that fetch
//! `__probe/<check id>`, so a test holds exactly one check's answer.

#![cfg(feature = "web")]

use std::io::Cursor;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use agentnative::web_audit::credential::{Credential, TokenSource};
use agentnative::web_audit::engine::antecedents::is_source_check;
use agentnative::web_audit::engine::sign_in::is_wire_probe;
use agentnative::web_audit::engine::{
    FetchHandle, HandlerContext, HandlerSet, ProbeOutcome, ProbeStatus, ProgressSink, RunInput,
    RunOutcome, RunReport, run_web_audit,
};
use agentnative::web_audit::fetch::FetchInit;
use agentnative::web_audit::headers::Headers;
use agentnative::web_audit::locality::Resolver;
use agentnative::web_audit::registry::{CHECKS, HANDLER_KINDS, WebCheck};
use agentnative::web_audit::scorecard::{EngineResult, EvidenceItem, ScorecardStatus};
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};
use serde_json::{Value, json};

const BASE: &str = "https://example.com/";

/// How long a held answer waits for what it is watching before giving up, so
/// an engine that never starts it fails the assertion instead of hanging.
const GIVE_UP: Duration = Duration::from_secs(1);

struct PublicResolver;

impl Resolver for PublicResolver {
    fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
        Ok(vec!["93.184.216.34".parse().unwrap()])
    }
}

/// A latch that opens by itself after [`GIVE_UP`].
#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn wait(&self) {
        let until = Instant::now() + GIVE_UP;
        let mut open = self.open.lock().unwrap();
        while !*open {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return;
            }
            open = self.changed.wait_timeout(open, left).unwrap().0;
        }
    }

    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

type Answer = dyn Fn(&Request, &str) -> Option<Response> + Send + Sync;

/// HTML at the root, 404 for anything `answer` does not take.
struct Site {
    answer: Box<Answer>,
}

impl Transport for Site {
    fn send(&self, request: &Request) -> Result<Response, TransportError> {
        let path = url::Url::parse(&request.url)
            .expect("a parsed URL")
            .path()
            .to_string();
        if let Some(response) = (self.answer)(request, &path) {
            return Ok(response);
        }
        Ok(match path.as_str() {
            "/" => respond(
                200,
                "text/html",
                "<!doctype html><html><body>hi</body></html>",
            ),
            _ if path.starts_with("/__probe/") => respond(200, "application/json", "{}"),
            _ => respond(404, "text/plain", ""),
        })
    }
}

fn respond(status: u16, content_type: &str, body: &str) -> Response {
    Response {
        status,
        headers: Headers::from_pairs([("content-type", content_type)]),
        body: Box::new(Cursor::new(body.as_bytes().to_vec())),
    }
}

fn card() -> Response {
    respond(200, "application/json", r#"{"mcp_endpoint":"/mcp"}"#)
}

fn site(
    answer: impl Fn(&Request, &str) -> Option<Response> + Send + Sync + 'static,
) -> Arc<dyn Transport + Send + Sync> {
    Arc::new(Site {
        answer: Box::new(answer),
    })
}

fn probe(check: &WebCheck, ctx: &HandlerContext) -> ProbeOutcome {
    let url = format!("{}__probe/{}", ctx.base, check.id);
    let resp = ctx
        .fetch
        .fetch(&url, &FetchInit::default(), &ctx.fetch_options(None));
    let Some(status) = resp.status else {
        return ProbeOutcome::error(resp.error.as_deref().unwrap_or("fetch failed"));
    };
    let mut item: EvidenceItem = serde_json::from_str(&resp.body).unwrap_or_default();
    item.insert("status".to_string(), json!(status));
    item.entry("url").or_insert(json!(url));
    let verdict = if status == 200 {
        ProbeStatus::Pass
    } else {
        ProbeStatus::Absent
    };
    ProbeOutcome::new(verdict, vec![item])
}

fn stand_ins() -> HandlerSet {
    let mut set = HandlerSet::new();
    for kind in HANDLER_KINDS {
        set.register(kind, probe);
    }
    for rule in [
        "api-description",
        "legacy-alias-redirects",
        "retained-document",
    ] {
        set.register_eval(rule, probe);
    }
    set
}

fn input<'a>(
    transport: Arc<dyn Transport + Send + Sync>,
    handlers: HandlerSet,
    concurrency: usize,
) -> RunInput<'a> {
    let fetch = FetchHandle::new(transport, Arc::new(PublicResolver), "test-ua");
    let mut input = RunInput::new(BASE, fetch, Arc::new(handlers));
    input.per_check_timeout = Duration::from_secs(3);
    input.per_audit_deadline = Duration::from_secs(20);
    input.concurrency = concurrency;
    input
}

fn complete(outcome: RunOutcome) -> Box<RunReport> {
    match outcome {
        RunOutcome::Complete(report) => report,
        RunOutcome::Unreachable { reason, .. } => panic!("unreachable: {reason}"),
    }
}

fn row<'a>(report: &'a RunReport, id: &str) -> &'a EngineResult {
    report
        .results
        .iter()
        .find(|r| r.check.id == id)
        .unwrap_or_else(|| panic!("no row {id}"))
}

#[test]
fn a_check_with_nothing_to_read_starts_while_a_source_is_still_reading() {
    let seen = Arc::new(Gate::default());
    let open = Arc::new(AtomicBool::new(false));
    let started_while_open = Arc::new(AtomicBool::new(false));
    let (s, o, w) = (seen.clone(), open.clone(), started_while_open.clone());
    let transport = site(move |_, path| {
        match path {
            "/__probe/llms-full-txt" => {
                o.store(true, Ordering::SeqCst);
                s.wait();
                o.store(false, Ordering::SeqCst);
            }
            "/__probe/security-txt" => {
                w.store(o.load(Ordering::SeqCst), Ordering::SeqCst);
                s.open();
            }
            _ => {}
        }
        None
    });
    complete(run_web_audit(input(transport, stand_ins(), 6)));
    assert!(started_while_open.load(Ordering::SeqCst));
}

#[test]
fn a_dependent_check_starts_once_its_own_source_settles_while_an_unrelated_source_reads() {
    let seen = Arc::new(Gate::default());
    let open = Arc::new(AtomicBool::new(false));
    let robots_answered = Arc::new(AtomicBool::new(false));
    let watched = Arc::new(Mutex::new((false, false)));
    let (s, o, r, w) = (
        seen.clone(),
        open.clone(),
        robots_answered.clone(),
        watched.clone(),
    );
    let transport = site(move |_, path| {
        match path {
            "/__probe/llms-full-txt" => {
                o.store(true, Ordering::SeqCst);
                s.wait();
                o.store(false, Ordering::SeqCst);
            }
            "/__probe/robots" => {
                thread::sleep(Duration::from_millis(5));
                r.store(true, Ordering::SeqCst);
            }
            "/__probe/robots-ai-rules" => {
                *w.lock().unwrap() = (o.load(Ordering::SeqCst), r.load(Ordering::SeqCst));
                s.open();
            }
            _ => {}
        }
        None
    });
    let report = complete(run_web_audit(input(transport, stand_ins(), 6)));
    assert_eq!(*watched.lock().unwrap(), (true, true));
    assert_eq!(
        row(&report, "robots-ai-rules").status,
        ScorecardStatus::Pass
    );
}

#[test]
fn a_check_waits_for_the_source_its_handler_reads_when_its_antecedent_reads_nothing() {
    let mut set = stand_ins();
    set.register("content-without-js", |check, ctx| {
        let mut outcome = probe(check, ctx);
        outcome.evidence[0].insert(
            "retained".to_string(),
            json!(ctx.retained_bodies.get("llms-txt")),
        );
        outcome
    });
    let transport = site(|_, path| {
        (path == "/__probe/llms-txt").then(|| {
            thread::sleep(Duration::from_millis(40));
            respond(200, "application/json", r##"{"body":"# Site\n"}"##)
        })
    });
    let report = complete(run_web_audit(input(transport, set, 6)));
    assert_eq!(
        row(&report, "content-without-js").raw_evidence[0].get("retained"),
        Some(&json!("# Site\n"))
    );
}

struct ProgressOf {
    on_result: Box<dyn FnMut(&EngineResult)>,
}

impl ProgressSink for ProgressOf {
    fn result(&mut self, result: &EngineResult) {
        (self.on_result)(result);
    }
}

#[test]
fn a_result_reaches_the_consumer_while_an_unrelated_source_still_reads() {
    let release = Arc::new(Gate::default());
    let finished = Arc::new(AtomicBool::new(false));
    let (rel, fin) = (release.clone(), finished.clone());
    let transport = site(move |_, path| {
        if path == "/__probe/llms-full-txt" {
            rel.wait();
            fin.store(true, Ordering::SeqCst);
        }
        None
    });
    let first_before_source = Arc::new(Mutex::new(None));
    let (first, fin, rel) = (
        first_before_source.clone(),
        finished.clone(),
        release.clone(),
    );
    let mut progress = ProgressOf {
        on_result: Box::new(move |_| {
            let mut first = first.lock().unwrap();
            if first.is_none() {
                *first = Some(!fin.load(Ordering::SeqCst));
                rel.open();
            }
        }),
    };
    let mut run = input(transport, stand_ins(), 6);
    run.progress = Some(&mut progress);
    complete(run_web_audit(run));
    assert_eq!(*first_before_source.lock().unwrap(), Some(true));
}

#[test]
fn a_check_its_gate_rules_out_takes_no_slot_and_reports_while_every_slot_is_held() {
    // With one slot the sources run in registry order, and `sitemap` follows
    // `robots`, so the row gated on `robots` has no slot to take.
    let release = Arc::new(Gate::default());
    let held = Arc::new(AtomicBool::new(false));
    let (rel, h) = (release.clone(), held.clone());
    let transport = site(move |_, path| match path {
        "/__probe/robots" => Some(respond(404, "text/plain", "")),
        "/__probe/sitemap" => {
            h.store(true, Ordering::SeqCst);
            rel.wait();
            h.store(false, Ordering::SeqCst);
            None
        }
        _ => None,
    });
    let gated_while_held = Arc::new(Mutex::new(None));
    let (gated, h, rel) = (gated_while_held.clone(), held.clone(), release.clone());
    let mut progress = ProgressOf {
        on_result: Box::new(move |result| {
            if result.check.id == "robots-ai-rules" {
                // The next source takes the slot as this row reports. Past the
                // wait, the row reported only because the slot came free.
                let until = Instant::now() + Duration::from_millis(300);
                while !h.load(Ordering::SeqCst) && Instant::now() < until {
                    thread::sleep(Duration::from_millis(1));
                }
                *gated.lock().unwrap() =
                    Some(h.load(Ordering::SeqCst) && result.status == ScorecardStatus::NA);
                rel.open();
            }
        }),
    };
    let mut run = input(transport, stand_ins(), 1);
    run.progress = Some(&mut progress);
    complete(run_web_audit(run));
    assert_eq!(*gated_while_held.lock().unwrap(), Some(true));
}

#[test]
fn every_later_mcp_request_follows_the_notification_and_a_check_outside_mcp_does_not_wait() {
    let seen = Arc::new(Gate::default());
    let notifying = Arc::new(AtomicBool::new(false));
    let notified = Arc::new(AtomicBool::new(false));
    let other_while_notifying = Arc::new(AtomicBool::new(false));
    let tools_after_notified = Arc::new(AtomicBool::new(false));
    let (s, ing, ed, other, tools) = (
        seen.clone(),
        notifying.clone(),
        notified.clone(),
        other_while_notifying.clone(),
        tools_after_notified.clone(),
    );
    let transport = site(move |request, path| match path {
        "/.well-known/mcp.json" => Some(card()),
        "/__probe/mcp-initialize" => Some(respond(
            200,
            "application/json",
            r#"{"session_id":"sess-1"}"#,
        )),
        "/mcp" => {
            ing.store(true, Ordering::SeqCst);
            s.wait();
            ing.store(false, Ordering::SeqCst);
            ed.store(true, Ordering::SeqCst);
            assert_eq!(request.method, "POST");
            Some(respond(202, "text/plain", ""))
        }
        "/__probe/robots" => {
            // Later than the wire probes, so the row it opens can land inside
            // the notification.
            thread::sleep(Duration::from_millis(30));
            None
        }
        "/__probe/robots-ai-rules" => {
            other.store(ing.load(Ordering::SeqCst), Ordering::SeqCst);
            s.open();
            None
        }
        "/__probe/mcp-tools-list" => {
            tools.store(ed.load(Ordering::SeqCst), Ordering::SeqCst);
            None
        }
        _ => None,
    });
    complete(run_web_audit(input(transport, stand_ins(), 6)));
    assert!(other_while_notifying.load(Ordering::SeqCst));
    assert!(tools_after_notified.load(Ordering::SeqCst));
}

#[test]
fn a_server_with_no_session_is_sent_no_notification_and_its_rows_still_run() {
    let posts = Arc::new(AtomicUsize::new(0));
    let tools = Arc::new(AtomicBool::new(false));
    let (p, t) = (posts.clone(), tools.clone());
    let transport = site(move |_, path| match path {
        "/.well-known/mcp.json" => Some(card()),
        "/mcp" => {
            p.fetch_add(1, Ordering::SeqCst);
            None
        }
        "/__probe/mcp-tools-list" => {
            t.store(true, Ordering::SeqCst);
            None
        }
        _ => None,
    });
    let report = complete(run_web_audit(input(transport, stand_ins(), 6)));
    assert_eq!(posts.load(Ordering::SeqCst), 0);
    assert!(tools.load(Ordering::SeqCst));
    assert!(report.complete);
}

#[test]
fn a_credentialed_runs_re_ask_starts_when_the_wire_probes_finish_and_sign_in_waits_for_it() {
    let seen = Arc::new(Gate::default());
    let open = Arc::new(AtomicBool::new(false));
    let re_ask_while_open = Arc::new(AtomicBool::new(false));
    let asked = Arc::new(AtomicUsize::new(0));
    let asked_when_row_ran = Arc::new(AtomicUsize::new(usize::MAX));
    let mut set = stand_ins();
    {
        let (s, o, w, a, r) = (
            seen.clone(),
            open.clone(),
            re_ask_while_open.clone(),
            asked.clone(),
            asked_when_row_ran.clone(),
        );
        set.register("mcp", move |check, ctx| {
            if is_wire_probe(check.id) && !ctx.fetch.credentialed() {
                // Started beside the wire probes, the held source may not have
                // reached the transport yet; an engine that waited for it to end
                // never sees it open.
                let until = Instant::now() + Duration::from_millis(300);
                while !o.load(Ordering::SeqCst) && Instant::now() < until {
                    thread::sleep(Duration::from_millis(1));
                }
                w.fetch_or(o.load(Ordering::SeqCst), Ordering::SeqCst);
                s.open();
                thread::sleep(Duration::from_millis(10));
                let outcome = probe(check, ctx);
                a.fetch_add(1, Ordering::SeqCst);
                return outcome;
            }
            if check.id == "mcp-tools-list" {
                r.store(a.load(Ordering::SeqCst), Ordering::SeqCst);
            }
            probe(check, ctx)
        });
    }
    let (s, o) = (seen.clone(), open.clone());
    let transport = site(move |_, path| {
        match path {
            "/.well-known/mcp.json" => return Some(card()),
            "/__probe/llms-full-txt" => {
                o.store(true, Ordering::SeqCst);
                s.wait();
                o.store(false, Ordering::SeqCst);
            }
            _ => {}
        }
        None
    });
    let token = Credential::bearer("fixture-token", TokenSource::Stdin).expect("a valid token");
    let mut run = input(transport, set, 6);
    run.fetch = run.fetch.with_credential(token);
    complete(run_web_audit(run));
    assert!(
        re_ask_while_open.load(Ordering::SeqCst),
        "the re-ask waited for a source it does not read"
    );
    assert_eq!(asked_when_row_ran.load(Ordering::SeqCst), 2);
}

#[test]
fn the_scorecard_lists_rows_in_registry_order_however_the_probes_finish() {
    let transport = site(|_, path| {
        if path == "/__probe/openapi" {
            thread::sleep(Duration::from_millis(30));
        }
        None
    });
    let reported = Arc::new(Mutex::new(Vec::new()));
    let order = reported.clone();
    let mut progress = ProgressOf {
        on_result: Box::new(move |result| order.lock().unwrap().push(result.check.id)),
    };
    let mut run = input(transport, stand_ins(), 6);
    run.progress = Some(&mut progress);
    let report = complete(run_web_audit(run));
    let registry: Vec<&str> = CHECKS.iter().map(|c| c.id).collect();
    let rows: Vec<&str> = report.results.iter().map(|r| r.check.id).collect();
    assert_eq!(rows, registry);
    let scored: Vec<&str> = report
        .scorecard
        .results
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(scored, registry);
    assert_ne!(*reported.lock().unwrap(), registry);
}

fn assert_peak_in_flight(cap: usize) {
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let (r, p) = (running.clone(), peak.clone());
    let transport = site(move |_, path| {
        if path.starts_with("/__probe/") {
            let now = r.fetch_add(1, Ordering::SeqCst) + 1;
            p.fetch_max(now, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(3));
            r.fetch_sub(1, Ordering::SeqCst);
        }
        None
    });
    complete(run_web_audit(input(transport, stand_ins(), cap)));
    assert_eq!(peak.load(Ordering::SeqCst), cap);
}

#[test]
fn no_more_than_the_cap_are_in_flight_sources_and_dependents_together() {
    for cap in [1, 3, 6] {
        assert_peak_in_flight(cap);
    }
}

#[test]
fn a_free_slot_goes_to_mcp_rows_ahead_of_later_rows_that_were_ready_first() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let seen = order.clone();
    let transport = site(move |_, path| {
        if path == "/.well-known/mcp.json" {
            return Some(card());
        }
        if let Some(id) = path.strip_prefix("/__probe/") {
            seen.lock().unwrap().push(id.to_string());
            thread::sleep(Duration::from_millis(2));
        }
        None
    });
    complete(run_web_audit(input(transport, stand_ins(), 2)));
    let order = order.lock().unwrap();
    let index = |id: &str| CHECKS.iter().position(|check| check.id == id);
    let tools = index("mcp-tools-list").expect("a registry check");
    let started_at = order
        .iter()
        .position(|id| id == "mcp-tools-list")
        .expect("the row ran");
    // Every row after it in the registry that reads nothing was ready before it.
    // Both slots can be taken by such rows as the sign-in step runs, and no more.
    let ahead: Vec<&String> = order[..started_at]
        .iter()
        .filter(|id| index(id).is_some_and(|i| i > tools) && !is_source_check(id))
        .collect();
    assert!(ahead.len() <= 2, "{ahead:?}");
}

#[test]
fn a_source_whose_handler_panics_keeps_its_error_row_and_what_is_gated_on_it_reads_not_applicable()
{
    let mut set = stand_ins();
    set.register("http", |check, ctx| {
        assert_ne!(check.id, "llms-full-txt", "this source's handler throws");
        probe(check, ctx)
    });
    let transport =
        site(|_, path| (path == "/__probe/llms-txt").then(|| respond(404, "text/plain", "")));
    let report = complete(run_web_audit(input(transport, set, 6)));
    let source = row(&report, "llms-full-txt");
    assert_eq!(source.status, ScorecardStatus::Error, "{source:?}");
    assert!(
        source.evidence.contains("this source's handler throws"),
        "{source:?}"
    );
    assert_eq!(
        row(&report, "llms-full-txt-scoped").status,
        ScorecardStatus::NA
    );
}

#[test]
fn a_source_still_out_at_the_deadline_keeps_its_skip_row_and_what_is_gated_on_it_reads_not_applicable()
 {
    let mut set = stand_ins();
    set.register("http", |check, ctx| {
        if check.id == "llms-full-txt" {
            thread::sleep(Duration::from_millis(600));
        }
        probe(check, ctx)
    });
    let transport =
        site(|_, path| (path == "/__probe/llms-txt").then(|| respond(404, "text/plain", "")));
    let mut run = input(transport, set, 6);
    run.per_audit_deadline = Duration::from_millis(250);
    let report = complete(run_web_audit(run));
    let source = row(&report, "llms-full-txt");
    assert_eq!(source.status, ScorecardStatus::Skip, "{source:?}");
    assert_eq!(
        row(&report, "llms-full-txt-scoped").status,
        ScorecardStatus::NA
    );
    assert!(!report.complete);
}

#[test]
fn a_handler_panic_is_that_rows_error_and_the_run_goes_on() {
    let mut set = stand_ins();
    set.register("mcp", |check, ctx| {
        assert_ne!(check.id, "mcp-tools-list", "a handler panics");
        probe(check, ctx)
    });
    let transport = site(|_, path| (path == "/.well-known/mcp.json").then(card));
    let report = complete(run_web_audit(input(transport, set, 6)));
    let broken = row(&report, "mcp-tools-list");
    assert_eq!(broken.status, ScorecardStatus::Error);
    assert!(broken.evidence.starts_with("handler error: "), "{broken:?}");
    assert_eq!(report.results.len(), CHECKS.len());
    assert_eq!(
        row(&report, "mcp-capabilities").raw_evidence[0].get("status"),
        Some(&Value::from(200))
    );
}
