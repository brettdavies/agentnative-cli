//! The root fetch and discovery's first document reads run side by side, and
//! what the root turns out to be decides how long the reads keep: a root that
//! fails at the network level cuts them to the degraded budget, and one that
//! redirects to http ends the run. A stalling host costs no more than it would
//! with the reads waiting their turn.

#![cfg(feature = "web")]

use std::io::Cursor;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use agentnative::web_audit::engine::{
    DEGRADED_PER_CHECK_TIMEOUT, FetchHandle, HandlerSet, RunInput, RunOutcome, run_web_audit,
};
use agentnative::web_audit::headers::Headers;
use agentnative::web_audit::locality::Resolver;
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};

const BASE: &str = "https://example.com/";
const DOCUMENT: &str = "/.well-known/mcp.json";

/// How long a held answer waits for what it is watching before giving up, so
/// an engine that never starts it fails the assertion instead of hanging.
const GIVE_UP: Duration = Duration::from_secs(1);

struct PublicResolver;

impl Resolver for PublicResolver {
    fn resolve(&self, _host: &str) -> Result<Vec<IpAddr>, String> {
        Ok(vec!["93.184.216.34".parse().unwrap()])
    }
}

#[derive(Default)]
struct Latch {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Latch {
    /// Whether the latch opened in time.
    fn wait(&self, at_most: Duration) -> bool {
        let until = Instant::now() + at_most;
        let mut open = self.open.lock().unwrap();
        while !*open {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            open = self.changed.wait_timeout(open, left).unwrap().0;
        }
        true
    }

    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
}

type Answer = dyn Fn(&str) -> Result<Response, TransportError> + Send + Sync;

struct Site {
    answer: Box<Answer>,
    seen: Mutex<Vec<String>>,
}

impl Transport for Site {
    fn send(&self, request: &Request) -> Result<Response, TransportError> {
        self.seen.lock().unwrap().push(request.url.clone());
        let path = url::Url::parse(&request.url).unwrap().path().to_string();
        (self.answer)(&path)
    }
}

fn site(
    answer: impl Fn(&str) -> Result<Response, TransportError> + Send + Sync + 'static,
) -> Arc<Site> {
    Arc::new(Site {
        answer: Box::new(answer),
        seen: Mutex::new(Vec::new()),
    })
}

fn answer(status: u16, headers: &[(&str, &str)], body: &str) -> Result<Response, TransportError> {
    Ok(Response {
        status,
        headers: Headers::from_pairs(headers.iter().copied()),
        body: Box::new(Cursor::new(body.as_bytes().to_vec())),
    })
}

fn html() -> Result<Response, TransportError> {
    answer(200, &[("content-type", "text/html")], "<html></html>")
}

fn refused() -> Result<Response, TransportError> {
    Err(TransportError::Verbatim("connection refused".to_string()))
}

fn input<'a>(transport: Arc<Site>) -> RunInput<'a> {
    let fetch = FetchHandle::new(transport, Arc::new(PublicResolver), "test-ua");
    let mut input = RunInput::new(BASE, fetch, Arc::new(HandlerSet::new()));
    input.per_check_timeout = Duration::from_secs(8);
    input.per_audit_deadline = Duration::from_secs(25);
    input
}

#[test]
fn a_document_read_goes_out_while_the_root_is_unanswered_and_the_root_reads_while_it_is() {
    // Each holds its answer until the other arrives, which only happens if both
    // are out at once.
    let root_out = Arc::new(Latch::default());
    let document_out = Arc::new(Latch::default());
    let root_saw_document = Arc::new(AtomicBool::new(false));
    let document_saw_root = Arc::new(AtomicBool::new(false));
    let (ro, dout, rs, ds) = (
        root_out.clone(),
        document_out.clone(),
        root_saw_document.clone(),
        document_saw_root.clone(),
    );
    let transport = site(move |path| match path {
        "/" => {
            ro.release();
            rs.store(dout.wait(GIVE_UP), Ordering::SeqCst);
            html()
        }
        DOCUMENT => {
            dout.release();
            ds.store(ro.wait(GIVE_UP), Ordering::SeqCst);
            answer(404, &[], "")
        }
        _ => answer(404, &[], ""),
    });
    let outcome = run_web_audit(input(transport));
    assert!(matches!(outcome, RunOutcome::Complete(_)));
    assert!(root_saw_document.load(Ordering::SeqCst));
    assert!(document_saw_root.load(Ordering::SeqCst));
}

#[test]
fn a_redirect_hop_of_a_first_read_waits_for_the_root_and_the_wait_is_not_its_budget() {
    let timeout = Duration::from_millis(500);
    let root_answered = Arc::new(AtomicBool::new(false));
    let hop_before_root = Arc::new(AtomicBool::new(false));
    let hop_status = Arc::new(Mutex::new(None));
    let (answered, before) = (root_answered.clone(), hop_before_root.clone());
    let transport = site(move |path| match path {
        "/" => {
            thread::sleep(Duration::from_millis(350));
            answered.store(true, Ordering::SeqCst);
            html()
        }
        DOCUMENT => answer(302, &[("location", "/moved")], ""),
        "/moved" => {
            before.store(!answered.load(Ordering::SeqCst), Ordering::SeqCst);
            // Longer than what the read would have left had the wait spent it.
            thread::sleep(Duration::from_millis(250));
            answer(404, &[], "")
        }
        _ => answer(404, &[], ""),
    });
    let mut run = input(transport.clone());
    run.per_check_timeout = timeout;
    let outcome = run_web_audit(run);
    let RunOutcome::Complete(report) = outcome else {
        panic!("the root answered");
    };
    assert!(!hop_before_root.load(Ordering::SeqCst));
    let trail: Vec<_> = report
        .discovery
        .evidence
        .iter()
        .filter(|item| item.get("source").and_then(|s| s.as_str()) == Some(DOCUMENT))
        .collect();
    *hop_status.lock().unwrap() = trail.first().and_then(|item| item.get("status").cloned());
    assert_eq!(
        *hop_status.lock().unwrap(),
        Some(serde_json::json!(404)),
        "the wait for the root spent the read's budget: {trail:?}"
    );
    assert!(
        transport
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|url| url.ends_with("/moved")),
        "the hop never went out"
    );
}

#[test]
fn a_root_redirecting_to_http_means_a_held_redirect_hop_is_never_sent() {
    let transport = site(move |path| match path {
        "/" => {
            thread::sleep(Duration::from_millis(40));
            answer(301, &[("location", "http://example.com/")], "")
        }
        DOCUMENT => answer(302, &[("location", "/moved")], ""),
        _ => answer(404, &[], ""),
    });
    let outcome = run_web_audit(input(transport.clone()));
    assert!(matches!(outcome, RunOutcome::Unreachable { .. }));
    thread::sleep(Duration::from_millis(30));
    let asked = transport.seen.lock().unwrap();
    assert!(
        !asked.iter().any(|url| url.ends_with("/moved")),
        "{asked:?}"
    );
}

#[test]
fn a_root_failing_at_the_network_level_cuts_a_read_in_flight_to_the_degraded_budget() {
    let release = Arc::new(Latch::default());
    let held = release.clone();
    let transport = site(move |path| match path {
        "/" => refused(),
        DOCUMENT => {
            held.wait(Duration::from_secs(30));
            answer(404, &[], "")
        }
        _ => answer(404, &[], ""),
    });
    let started = Instant::now();
    let outcome = run_web_audit(input(transport));
    let elapsed = started.elapsed();
    release.release();
    assert!(matches!(outcome, RunOutcome::Complete(_)));
    assert!(
        elapsed >= DEGRADED_PER_CHECK_TIMEOUT - Duration::from_millis(50),
        "{elapsed:?}"
    );
    assert!(
        elapsed < DEGRADED_PER_CHECK_TIMEOUT + Duration::from_millis(1_500),
        "the read kept its configured budget: {elapsed:?}"
    );
}

#[test]
fn a_target_that_answers_nothing_costs_its_root_timeout_not_that_plus_a_turn_for_the_reads() {
    let timeout = Duration::from_millis(400);
    let release = Arc::new(Latch::default());
    let held = release.clone();
    let transport = site(move |_| {
        held.wait(Duration::from_secs(30));
        answer(404, &[], "")
    });
    let mut run = input(transport);
    run.per_check_timeout = timeout;
    let started = Instant::now();
    let outcome = run_web_audit(run);
    let elapsed = started.elapsed();
    release.release();
    assert!(matches!(outcome, RunOutcome::Unreachable { .. }));
    // The root and the first reads together, then the common-path POSTs.
    assert!(
        elapsed < timeout * 3 - Duration::from_millis(100),
        "{elapsed:?}"
    );
}

#[test]
fn a_root_redirecting_to_http_ends_the_run_on_its_answer_and_stops_reads_in_flight() {
    let release = Arc::new(Latch::default());
    let held = release.clone();
    let transport = site(move |path| match path {
        "/" => answer(301, &[("location", "http://example.com/")], ""),
        DOCUMENT => {
            held.wait(Duration::from_secs(30));
            answer(404, &[], "")
        }
        _ => answer(404, &[], ""),
    });
    let started = Instant::now();
    let outcome = run_web_audit(input(transport.clone()));
    let elapsed = started.elapsed();
    release.release();
    let RunOutcome::Unreachable { reason, .. } = outcome else {
        panic!("a root redirecting to http scored");
    };
    assert!(reason.contains("redirects to http"), "{reason}");
    assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
    let asked = transport.seen.lock().unwrap();
    assert!(
        asked.iter().all(|url| !url.starts_with("http:")),
        "{asked:?}"
    );
}
