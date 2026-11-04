//! `llms-txt-links` probes side by side under the audit's cap. A link not
//! answered by the check's deadline reads as the budget running out on its own
//! row, whichever order the others finished in, and every other link still
//! counts.

#![cfg(feature = "web")]

mod common;

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use agentnative::web_audit::engine::slots::{AuditSlots, Rank};
use agentnative::web_audit::engine::{HandlerContext, ProbeOutcome, ProbeStatus};
use agentnative::web_audit::fetch::memo::RequestMemo;
use agentnative::web_audit::handlers::llms_txt_quality::run_llms_txt_quality;
use agentnative::web_audit::headers::Headers;
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};
use common::handler_context::{check, context};
use serde_json::{Value, json};

const LINK_COUNT: usize = 6;

fn hrefs() -> Vec<String> {
    (0..LINK_COUNT)
        .map(|i| format!("https://example.com/p{i}"))
        .collect()
}

fn llms_body() -> String {
    let links: Vec<String> = hrefs()
        .iter()
        .enumerate()
        .map(|(i, href)| format!("- [p{i}]({href})"))
        .collect();
    format!("# Site\n\n> Summary\n\n{}\n", links.join("\n"))
}

type Answer = dyn Fn(&str) -> Result<Response, TransportError> + Send + Sync;

struct Target {
    answer: Box<Answer>,
    asked: Mutex<Vec<String>>,
}

impl Transport for Target {
    fn send(&self, request: &Request) -> Result<Response, TransportError> {
        self.asked.lock().unwrap().push(request.url.clone());
        (self.answer)(&request.url)
    }
}

fn target(
    answer: impl Fn(&str) -> Result<Response, TransportError> + Send + Sync + 'static,
) -> Arc<Target> {
    Arc::new(Target {
        answer: Box::new(answer),
        asked: Mutex::new(Vec::new()),
    })
}

fn status(status: u16) -> Result<Response, TransportError> {
    Ok(Response {
        status,
        headers: Headers::new(),
        body: Box::new(Cursor::new(Vec::new())),
    })
}

fn hang() -> Result<Response, TransportError> {
    thread::sleep(Duration::from_secs(2));
    Err(TransportError::Timeout)
}

fn links_context(
    transport: Arc<Target>,
    slots: &AuditSlots,
    memo: &RequestMemo,
    timeout: Duration,
) -> HandlerContext {
    let mut ctx = context(transport);
    ctx.fetch = ctx.fetch.with_memo(memo.clone());
    ctx.default_timeout = timeout;
    ctx.retained_bodies = Arc::new(HashMap::from([("llms-txt".to_string(), llms_body())]));
    ctx.slots = Some(slots.clone());
    ctx
}

/// The handler on a slot of its own, as the schedule runs it.
fn run_on_slot(slots: &AuditSlots, ctx: HandlerContext) -> ProbeOutcome {
    let (tx, rx) = mpsc::channel();
    slots.submit(Rank::Row(0), move || {
        let _ = tx.send(run_llms_txt_quality(check("llms-txt-links"), &ctx));
    });
    rx.recv_timeout(Duration::from_secs(10))
        .expect("the check ends")
}

fn urls(outcome: &ProbeOutcome) -> Vec<Value> {
    outcome
        .evidence
        .iter()
        .map(|row| row["url"].clone())
        .collect()
}

#[test]
fn the_probes_run_under_the_cap_and_evidence_keeps_link_order_however_they_finish() {
    let all = hrefs();
    let order: Vec<String> = all.clone();
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let answered = Arc::new(Mutex::new(Vec::new()));
    let (r, p, a) = (running.clone(), peak.clone(), answered.clone());
    let transport = target(move |url| {
        let now = r.fetch_add(1, Ordering::SeqCst) + 1;
        p.fetch_max(now, Ordering::SeqCst);
        let place = order.iter().position(|href| href == url).unwrap();
        thread::sleep(Duration::from_millis(((LINK_COUNT - place) * 12) as u64));
        r.fetch_sub(1, Ordering::SeqCst);
        a.lock().unwrap().push(url.to_string());
        status(200)
    });
    let slots = AuditSlots::new(4);
    let memo = RequestMemo::new();
    let outcome = run_on_slot(
        &slots,
        links_context(transport, &slots, &memo, Duration::from_secs(5)),
    );
    memo.close();
    slots.close();
    // One row's fan-out always leaves a slot free.
    assert_eq!(peak.load(Ordering::SeqCst), 3);
    assert_ne!(*answered.lock().unwrap(), all);
    assert_eq!(
        urls(&outcome),
        all.iter().map(|h| json!(h)).collect::<Vec<_>>()
    );
    assert_eq!(outcome.status, ProbeStatus::Pass, "{outcome:?}");
}

#[test]
fn the_check_spends_its_own_slot_when_no_other_frees_so_a_cap_of_one_finishes_in_link_order() {
    let transport = target(|_| status(200));
    let slots = AuditSlots::new(1);
    let memo = RequestMemo::new();
    let outcome = run_on_slot(
        &slots,
        links_context(transport.clone(), &slots, &memo, Duration::from_secs(5)),
    );
    assert_eq!(outcome.status, ProbeStatus::Pass, "{outcome:?}");
    assert_eq!(*transport.asked.lock().unwrap(), hrefs());
    let (tx, rx) = mpsc::channel();
    slots.submit(Rank::Row(1), move || tx.send("slot came back").unwrap());
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(5)),
        Ok("slot came back")
    );
    memo.close();
    slots.close();
}

#[test]
fn a_link_unanswered_at_the_deadline_reads_as_the_budget_running_out_and_every_other_link_counts() {
    let all = hrefs();
    let (stuck, missing) = (all[2].clone(), all[4].clone());
    let transport = target(move |url| {
        if url == stuck {
            hang()
        } else {
            status(if url == missing { 404 } else { 200 })
        }
    });
    let slots = AuditSlots::new(6);
    let memo = RequestMemo::new();
    let outcome = run_on_slot(
        &slots,
        links_context(transport, &slots, &memo, Duration::from_millis(120)),
    );
    memo.close();
    slots.close();
    assert_eq!(
        urls(&outcome),
        all.iter().map(|h| json!(h)).collect::<Vec<_>>()
    );
    assert_eq!(
        Value::Object(outcome.evidence[2].clone()),
        json!({
            "url": all[2],
            "ok": false,
            "why": ["nested-probe budget exhausted"],
            "link_verdict": "error"
        })
    );
    assert_eq!(outcome.status, ProbeStatus::Absent, "{outcome:?}");
    assert!(outcome.incomplete);
}

#[test]
fn a_link_still_waiting_for_a_slot_at_the_deadline_sends_nothing_and_reads_the_same_way() {
    let transport = target(|_| hang());
    let slots = AuditSlots::new(1);
    let memo = RequestMemo::new();
    let outcome = run_on_slot(
        &slots,
        links_context(transport.clone(), &slots, &memo, Duration::from_millis(80)),
    );
    memo.close();
    slots.close();
    let asked = transport.asked.lock().unwrap();
    assert_eq!(asked[0], hrefs()[0]);
    assert!(asked.len() < LINK_COUNT, "{asked:?}");
    for row in &outcome.evidence {
        assert_eq!(
            row["why"],
            json!(["nested-probe budget exhausted"]),
            "{row:?}"
        );
    }
    assert_eq!(outcome.status, ProbeStatus::Error);
    assert!(outcome.incomplete);
}

#[test]
fn with_no_slots_to_share_the_links_go_one_at_a_time() {
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let (r, p) = (running.clone(), peak.clone());
    let transport = target(move |_| {
        let now = r.fetch_add(1, Ordering::SeqCst) + 1;
        p.fetch_max(now, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(2));
        r.fetch_sub(1, Ordering::SeqCst);
        status(200)
    });
    let mut ctx = context(transport);
    ctx.retained_bodies = Arc::new(HashMap::from([("llms-txt".to_string(), llms_body())]));
    let outcome = run_llms_txt_quality(check("llms-txt-links"), &ctx);
    assert_eq!(outcome.status, ProbeStatus::Pass);
    assert_eq!(peak.load(Ordering::SeqCst), 1);
}
