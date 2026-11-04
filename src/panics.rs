//! What a panic says, and the test-only way to cause one.

use std::any::Any;
use std::path::Path;
use std::sync::Mutex;

static LOCATION: Mutex<Option<String>> = Mutex::new(None);

/// Keep each panic's location for [`last_location`] and print nothing, so stderr
/// can carry a JSON envelope alone. Only the file name: a dependency's path holds
/// the builder's home directory, and the location goes in a public bug report.
pub fn hold_location_quietly() {
    std::panic::set_hook(Box::new(|info| {
        let location = info.location().map(|at| {
            let file = Path::new(at.file()).file_name();
            let file = file.map_or_else(|| at.file().into(), |name| name.to_string_lossy());
            format!("{file}:{}", at.line())
        });
        *LOCATION.lock().unwrap_or_else(|e| e.into_inner()) = location;
    }));
}

/// Where the last panic [`hold_location_quietly`] saw was raised.
pub fn last_location() -> Option<String> {
    LOCATION.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The message a panic carried, from the payload `catch_unwind` returns.
pub fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "a panic with no message".to_string())
}

/// Panics when `ANC_PANIC_PROBE` names `site`: `run` for the top of a run, a web
/// check's id for its handler, or an audit's id. The test profile always unwinds,
/// so this is how a release-profile build shows what a panic does.
#[cfg(feature = "panic-probe")]
pub fn probe(site: &str) {
    if std::env::var("ANC_PANIC_PROBE").is_ok_and(|named| named == site) {
        panic!("panic probe in {site}");
    }
}
