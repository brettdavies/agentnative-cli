//! What a panic says, and the test-only way to cause one.

use std::any::Any;

/// The message a panic carried, from the payload `catch_unwind` returns.
pub fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "a panic with no message".to_string())
}

/// Panics when `ANC_PANIC_PROBE` names `site`: `run` for the top of a run, or a
/// web check's id for its handler. The test profile always unwinds, so this is
/// how a release-profile build shows what a panic does.
#[cfg(feature = "panic-probe")]
pub fn probe(site: &str) {
    if std::env::var("ANC_PANIC_PROBE").is_ok_and(|named| named == site) {
        panic!("panic probe in {site}");
    }
}
