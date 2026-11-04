//! What the corpus pins that a scorecard cannot show: every scenario sends the
//! requests the site engine sent, and neither its scorecard nor that set moves
//! with the order answers arrive in or the number of slots. Requests go out on
//! threads here, so an order the mock's instant answers never show is the
//! difference between a schedule that follows its edges and one that passes by
//! luck. The orders are the site generator's own (`answerOrders`).

#![cfg(feature = "web")]

mod common;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use agentnative::web_audit::engine::RunOutcome;
use agentnative::web_audit::engine::antecedents::is_source_check;
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::mock::MockTransport;
use agentnative::web_audit::registry::CHECKS;
use agentnative::web_audit::scorecard::to_wire_json;
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};
use common::corpus::{
    Case, Golden, Replay, SentRequest, corpus_dir, load_cases, recorded_requests, replay_through,
    transport_for,
};

/// How long to hold an answer back, given the request and its place in the
/// order sent.
type Delay = dyn Fn(&Request, usize) -> Duration + Send + Sync;

/// Records through the mock on arrival and holds the answer back.
struct Reordering {
    inner: Arc<MockTransport>,
    delay: Arc<Delay>,
    sent: AtomicUsize,
}

impl Transport for Reordering {
    fn send(&self, request: &Request) -> Result<Response, TransportError> {
        let answer = self.inner.send(request);
        let index = self.sent.fetch_add(1, Ordering::SeqCst);
        thread::sleep((self.delay)(request, index));
        answer
    }
}

/// Longer than any burst an audit sends at once: discovery's first reads, its
/// POSTs, a follow fan-out, a full set of slots.
const REVERSAL_SPAN: usize = 8;

// The site's millisecond steps, widened: threads start further apart than
// promises do, and a step shorter than that gap reverses nothing.
const REVERSAL_STEP: Duration = Duration::from_millis(3);
const SOURCE_ANSWER_DELAY: Duration = Duration::from_millis(20);

fn later_requests_first() -> Arc<Delay> {
    Arc::new(|_, index| REVERSAL_STEP * (REVERSAL_SPAN - index % REVERSAL_SPAN) as u32)
}

fn source_paths() -> HashSet<String> {
    let mut paths = HashSet::new();
    for check in CHECKS.iter().filter(|c| is_source_check(c.id)) {
        let with: serde_json::Value = serde_json::from_str(check.with_json).expect("with is JSON");
        paths.extend(with["path"].as_str().map(str::to_string));
        if let Some(any) = with["path_any"].as_array() {
            paths.extend(any.iter().filter_map(|p| p.as_str().map(str::to_string)));
        }
    }
    paths
}

fn source_documents_last() -> Arc<Delay> {
    let paths = source_paths();
    assert!(paths.contains("/llms.txt"), "{paths:?}");
    Arc::new(move |request, _| {
        let path = url::Url::parse(&request.url).map(|u| u.path().to_string());
        if path.is_ok_and(|p| paths.contains(&p)) {
            SOURCE_ANSWER_DELAY
        } else {
            Duration::ZERO
        }
    })
}

const SLOW_ROOT_TIMEOUT: Duration = Duration::from_millis(1200);
const SLOW_ROOT_ANSWER: Duration = Duration::from_millis(1100);
const AFTER_SLOW_ROOT_ANSWER: Duration = Duration::from_millis(120);

fn root_just_inside_its_timeout() -> Arc<Delay> {
    Arc::new(|request, _| {
        // The canonical root fetch, which carries nothing but the audit's identity;
        // other rows ask for the root with headers of their own.
        let is_root = request.method == "GET"
            && request.headers.len() == 1
            && url::Url::parse(&request.url).is_ok_and(|url| url.path() == "/");
        if is_root {
            SLOW_ROOT_ANSWER
        } else {
            AFTER_SLOW_ROOT_ANSWER
        }
    })
}

fn golden_text(name: &str) -> String {
    let path = corpus_dir()
        .join("scenarios")
        .join(name)
        .join("scorecard.json");
    fs::read_to_string(path).expect("a golden scorecard")
}

fn output_differs(case: &Case, outcome: &RunOutcome) -> Option<String> {
    match (outcome, &case.golden) {
        (RunOutcome::Complete(report), Golden::Scorecard(_)) => {
            let actual = to_wire_json(&report.scorecard);
            let expected = golden_text(&case.name);
            (actual != expected).then(|| {
                let line = actual
                    .lines()
                    .zip(expected.lines())
                    .find(|(a, e)| a != e)
                    .map_or("the lengths differ".to_string(), |(a, e)| {
                        format!("expected {} actual {}", e.trim(), a.trim())
                    });
                format!("scorecard: {line}")
            })
        }
        (RunOutcome::Unreachable { reason, .. }, Golden::Unreachable(expected)) => {
            (reason != expected).then(|| format!("unreachable: {reason}"))
        }
        (RunOutcome::Complete(_), Golden::Unreachable(_)) => {
            Some("scored where the site reported unreachable".to_string())
        }
        (RunOutcome::Unreachable { reason, .. }, Golden::Scorecard(_)) => {
            Some(format!("unreachable where the site scored: {reason}"))
        }
    }
}

fn request_set_differs(case: &Case, transport: &MockTransport) -> Option<String> {
    let expected = recorded_requests(&case.name);
    let mut counts: BTreeMap<SentRequest, usize> = BTreeMap::new();
    for request in transport.requests() {
        *counts.entry(SentRequest::from(&request)).or_default() += 1;
    }
    let twice: Vec<String> = counts
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|(r, n)| format!("{} {} x{n}", r.method, r.url))
        .collect();
    let sent: BTreeSet<SentRequest> = counts.into_keys().collect();
    if sent == expected && twice.is_empty() {
        return None;
    }
    let extra: Vec<String> = sent
        .difference(&expected)
        .map(|r| format!("{} {}", r.method, r.url))
        .collect();
    let missing: Vec<String> = expected
        .difference(&sent)
        .map(|r| format!("{} {}", r.method, r.url))
        .collect();
    Some(format!(
        "request set: extra {extra:?}, missing {missing:?}, sent twice {twice:?}"
    ))
}

/// What one run of the corpus varies.
#[derive(Clone, Default)]
struct Schedule {
    delay: Option<Arc<Delay>>,
    concurrency: Option<usize>,
    per_check_timeout: Option<Duration>,
}

fn run_case(case: &Case, schedule: &Schedule) -> Vec<String> {
    let mock = transport_for(&case.scenario);
    let transport: Arc<dyn Transport + Send + Sync> = match &schedule.delay {
        Some(delay) => Arc::new(Reordering {
            inner: Arc::clone(&mock),
            delay: Arc::clone(delay),
            sent: AtomicUsize::new(0),
        }),
        None => mock.clone(),
    };
    let handlers = Arc::new(default_handlers());
    let outcome = replay_through(
        &case.scenario,
        handlers,
        transport,
        Replay {
            concurrency: schedule.concurrency,
            per_check_timeout: schedule.per_check_timeout,
            ..Replay::default()
        },
    );
    [
        output_differs(case, &outcome),
        request_set_differs(case, &mock),
    ]
    .into_iter()
    .flatten()
    .map(|problem| format!("{}: {problem}", case.name))
    .collect()
}

/// Every scenario, several at a time: these runs mostly wait on timers.
fn assert_every_scenario_holds(schedule: Schedule) {
    let cases = load_cases();
    let next = AtomicUsize::new(0);
    let problems = Mutex::new(Vec::new());
    thread::scope(|scope| {
        for _ in 0..16 {
            scope.spawn(|| {
                while let Some(case) = cases.get(next.fetch_add(1, Ordering::SeqCst)) {
                    let found = run_case(case, &schedule);
                    problems.lock().unwrap().extend(found);
                }
            });
        }
    });
    let problems = problems.into_inner().unwrap();
    assert!(
        problems.is_empty(),
        "{} problem(s):\n{}",
        problems.len(),
        problems.join("\n")
    );
}

#[test]
fn every_scenario_sends_the_site_request_set() {
    assert_every_scenario_holds(Schedule::default());
}

#[test]
fn every_scenario_holds_at_a_concurrency_of_one() {
    assert_every_scenario_holds(Schedule {
        concurrency: Some(1),
        ..Schedule::default()
    });
}

#[test]
fn every_scenario_holds_with_later_requests_answering_first() {
    assert_every_scenario_holds(Schedule {
        delay: Some(later_requests_first()),
        ..Schedule::default()
    });
}

#[test]
fn every_scenario_holds_with_the_source_documents_answering_last() {
    assert_every_scenario_holds(Schedule {
        delay: Some(source_documents_last()),
        ..Schedule::default()
    });
}

/// Reordering alone cannot run anything out of budget. Here the root takes nearly
/// its whole timeout and an answer takes longer than the root left, so a request
/// that spent its own budget waiting for the root loses its answer. The site runs
/// this at a 50 ms margin; threads wake later than timers fire, so this one is 100.
#[test]
fn every_scenario_holds_with_the_root_answering_just_inside_its_timeout() {
    assert_every_scenario_holds(Schedule {
        delay: Some(root_just_inside_its_timeout()),
        per_check_timeout: Some(SLOW_ROOT_TIMEOUT),
        ..Schedule::default()
    });
}

/// The set `requests.json` holds cannot show this, so it has a test of its
/// own: the protocol has a client send nothing else on a session until the
/// server has its `initialized` notification.
#[test]
fn no_request_carrying_the_session_goes_out_until_the_notification_has_settled() {
    struct Watching {
        inner: Arc<MockTransport>,
        notified: AtomicBool,
        carrying: Mutex<Vec<(String, bool)>>,
    }
    impl Transport for Watching {
        fn send(&self, request: &Request) -> Result<Response, TransportError> {
            let body = String::from_utf8_lossy(request.body.as_deref().unwrap_or(&[]));
            if body.contains("notifications/initialized") {
                // Long enough for a row that did not wait to have sent its request.
                thread::sleep(Duration::from_millis(30));
                let answer = self.inner.send(request);
                self.notified.store(true, Ordering::SeqCst);
                return answer;
            }
            let session = request
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("mcp-session-id"));
            if session {
                let before = !self.notified.load(Ordering::SeqCst);
                self.carrying
                    .lock()
                    .unwrap()
                    .push((request.url.clone(), before));
            }
            self.inner.send(request)
        }
    }
    let cases = load_cases();
    let case = cases
        .iter()
        .find(|case| case.name == "mcp-stateful-session")
        .expect("the stateful session scenario");
    let watching = Arc::new(Watching {
        inner: transport_for(&case.scenario),
        notified: AtomicBool::new(false),
        carrying: Mutex::new(Vec::new()),
    });
    let outcome = replay_through(
        &case.scenario,
        Arc::new(default_handlers()),
        watching.clone(),
        Replay::default(),
    );
    assert!(matches!(outcome, RunOutcome::Complete(_)));
    assert!(watching.notified.load(Ordering::SeqCst));
    let carrying = watching.carrying.lock().unwrap();
    assert!(!carrying.is_empty(), "no later row carried the session");
    let early: Vec<&String> = carrying
        .iter()
        .filter(|(_, before)| *before)
        .map(|(url, _)| url)
        .collect();
    assert!(early.is_empty(), "sent before the notification: {early:?}");
}
