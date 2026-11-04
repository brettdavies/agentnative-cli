//! A transport that panics, and one fetch through the request memo.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agentnative::web_audit::engine::FetchHandle;
use agentnative::web_audit::fetch::memo::RequestMemo;
use agentnative::web_audit::fetch::{FetchInit, FetchOptions};
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};

use super::corpus::PublicResolver;

pub struct Panics;

impl Transport for Panics {
    fn send(&self, _: &Request) -> Result<Response, TransportError> {
        panic!("transport bug");
    }
}

/// One fetch through the request memo, the way every fetch in a real run goes, and
/// the panic it raised on this thread.
pub fn panic_through_the_memo(
    transport: Arc<dyn Transport + Send + Sync>,
) -> (Box<dyn Any + Send>, Duration) {
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
    (panic, started.elapsed())
}
