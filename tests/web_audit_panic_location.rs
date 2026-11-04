//! Where a panic's report says it happened. A binary of its own, because the hook
//! and the locations it keeps belong to the process.

#![cfg(feature = "web")]

mod common;

use std::panic::catch_unwind;
use std::sync::Arc;

use agentnative::panics::{describe, keep_locations};
use common::panicking::{Panics, panic_through_the_memo};

#[test]
fn a_panic_reports_where_it_was_raised_whatever_panicked_since() {
    keep_locations(false);
    let (first, _) = panic_through_the_memo(Arc::new(Panics));
    let second = catch_unwind(|| panic!("a later bug")).unwrap_err();
    let (first, second) = (describe(&*first), describe(&*second));
    drop(std::panic::take_hook());
    assert!(
        first.starts_with("transport bug (tests/common/panicking.rs:"),
        "{first}"
    );
    assert!(
        second.starts_with("a later bug (tests/web_audit_panic_location.rs:"),
        "{second}"
    );
}
