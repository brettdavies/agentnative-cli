//! What a panic inside a web check costs. A panic on one of a handler's own
//! threads has to reach the handler's `catch_unwind` as itself, so the row says
//! what went wrong.

#![cfg(feature = "web")]

mod common;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use agentnative::panics::panic_message;
use agentnative::web_audit::engine::api_targets::{ApiHostTarget, ApiTargets, HostAnchor};
use agentnative::web_audit::handlers::api_hygiene::run_api_hygiene;
use agentnative::web_audit::transport::{Request, Response, Transport, TransportError};
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
