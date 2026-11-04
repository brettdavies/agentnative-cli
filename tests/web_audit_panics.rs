//! What a panic costs a web audit. A panic on any thread below a caller reaches
//! that caller as itself: a handler's `catch_unwind` makes it the row's error, and
//! the top of the run makes it the `panic` error. It never reads as a fetch that
//! failed, which would blame the target.

#![cfg(feature = "web")]

mod common;

use std::io::{self, Read};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agentnative::panics::panic_message;
use agentnative::web_audit::engine::api_targets::{ApiHostTarget, ApiTargets, HostAnchor};
use agentnative::web_audit::engine::{FetchHandle, RunInput, run_web_audit};
use agentnative::web_audit::fetch::memo::RequestMemo;
use agentnative::web_audit::fetch::{FetchInit, FetchOptions};
use agentnative::web_audit::handlers::api_hygiene::run_api_hygiene;
use agentnative::web_audit::handlers::default_handlers;
use agentnative::web_audit::headers::Headers;
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};
use common::corpus::PublicResolver;
use common::handler_context::{check, context};

struct Panics;

impl Transport for Panics {
    fn send(&self, _: &Request) -> Result<Response, TransportError> {
        panic!("transport bug");
    }
}

#[test]
fn a_host_probe_that_panics_keeps_its_own_message() {
    let mut ctx = context(Arc::new(Panics));
    ctx.api_targets = Some(Arc::new(ApiTargets {
        descriptions: Vec::new(),
        hosts: vec![ApiHostTarget {
            origin: "https://api.example.com".to_string(),
            anchors: vec![HostAnchor {
                url: "https://api.example.com/v1".to_string(),
                description: "https://api.example.com/openapi.json".to_string(),
            }],
            declared: false,
            unmet: None,
            plaintext: None,
        }],
    }));
    let panic = catch_unwind(AssertUnwindSafe(|| {
        run_api_hygiene(check("rate-limit-headers"), &ctx)
    }))
    .expect_err("the transport panics");
    assert_eq!(panic_message(&*panic), "transport bug");
}

#[test]
fn a_panic_payload_reads_as_its_message() {
    let owned = catch_unwind(|| panic!("owned {}", 1)).unwrap_err();
    assert_eq!(panic_message(&*owned), "owned 1");
    let borrowed = catch_unwind(|| panic!("borrowed")).unwrap_err();
    assert_eq!(panic_message(&*borrowed), "borrowed");
    let other = catch_unwind(|| std::panic::panic_any(7u8)).unwrap_err();
    assert_eq!(panic_message(&*other), "a panic with no message");
}

struct PanicsOnRead;

impl Read for PanicsOnRead {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("body bug");
    }
}

struct PanickingBody;

impl Transport for PanickingBody {
    fn send(&self, _: &Request) -> Result<Response, TransportError> {
        Ok(Response {
            status: 200,
            headers: Headers::new(),
            body: Box::new(PanicsOnRead),
        })
    }
}

/// One fetch through the request memo, the way every fetch in a real run goes,
/// and the message of the panic it raised on this thread.
fn panic_through_the_memo(transport: Arc<dyn Transport + Send + Sync>) -> (String, Duration) {
    let fetch = FetchHandle::new(transport, Arc::new(PublicResolver), "test-ua")
        .with_memo(RequestMemo::new());
    let opts = FetchOptions {
        timeout: Duration::from_secs(5),
        ..FetchOptions::default()
    };
    let started = Instant::now();
    let panic = catch_unwind(AssertUnwindSafe(|| {
        fetch.fetch("https://example.com/", &FetchInit::default(), &opts)
    }))
    .expect_err("the panic reaches the caller");
    (panic_message(&*panic), started.elapsed())
}

#[test]
fn a_panic_on_a_hop_thread_reaches_the_caller_as_itself() {
    let (message, took) = panic_through_the_memo(Arc::new(Panics));
    assert_eq!(message, "transport bug");
    assert!(took < Duration::from_secs(4), "{took:?}");
}

#[test]
fn a_panic_in_a_body_read_reaches_the_caller_as_itself() {
    let (message, took) = panic_through_the_memo(Arc::new(PanickingBody));
    assert_eq!(message, "body bug");
    assert!(took < Duration::from_secs(4), "{took:?}");
}

#[test]
fn a_panic_on_every_probe_ends_the_run_rather_than_calling_the_target_unreachable() {
    let fetch = FetchHandle::new(Arc::new(Panics), Arc::new(PublicResolver), "test-ua");
    let input = RunInput::new("https://example.com/", fetch, Arc::new(default_handlers()));
    let panic = catch_unwind(AssertUnwindSafe(|| run_web_audit(input)))
        .expect_err("the run ends in the panic");
    assert_eq!(panic_message(&*panic), "transport bug");
}
