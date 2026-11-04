//! Where a panic's report says it happened. A binary of its own, because the hook
//! and the location it keeps belong to the process.

#![cfg(feature = "web")]

mod common;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::Duration;

use agentnative::panics::{hold_location_quietly, last_location};
use agentnative::web_audit::engine::FetchHandle;
use agentnative::web_audit::fetch::memo::RequestMemo;
use agentnative::web_audit::fetch::{FetchInit, FetchOptions};
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};
use common::corpus::PublicResolver;

struct Panics;

impl Transport for Panics {
    fn send(&self, _: &Request) -> Result<Response, TransportError> {
        panic!("transport bug");
    }
}

#[test]
fn a_panic_through_the_memo_reports_where_it_was_raised() {
    hold_location_quietly();
    let fetch = FetchHandle::new(Arc::new(Panics), Arc::new(PublicResolver), "test-ua")
        .with_memo(RequestMemo::new());
    let opts = FetchOptions {
        timeout: Duration::from_secs(5),
        ..FetchOptions::default()
    };
    catch_unwind(AssertUnwindSafe(|| {
        fetch.fetch("https://example.com/", &FetchInit::default(), &opts)
    }))
    .expect_err("the transport panics");
    let location = last_location().expect("the hook kept it");
    drop(std::panic::take_hook());
    assert!(
        location.starts_with("tests/web_audit_panic_location.rs:"),
        "{location}"
    );
}
