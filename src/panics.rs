//! What a panic says, and the test-only way to cause one.

use std::any::Any;
use std::borrow::Cow;
use std::path::Path;
use std::sync::Mutex;

static LOCATION: Mutex<Option<String>> = Mutex::new(None);

/// The path a bug report shows, which is public: a dependency's is absolute and
/// holds the builder's home directory, so it keeps only its file name, and this
/// crate's own is relative already.
fn reported(file: &str) -> Cow<'_, str> {
    let path = Path::new(file);
    match path.file_name() {
        Some(name) if path.is_absolute() => name.to_string_lossy(),
        _ => file.into(),
    }
}

/// Keep each panic's location for [`last_location`] and print nothing, so stderr
/// can carry a JSON envelope alone.
pub fn hold_location_quietly() {
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|at| format!("{}:{}", reported(at.file()), at.line()));
        *LOCATION.lock().unwrap_or_else(|e| e.into_inner()) = location;
    }));
}

/// Where the last panic [`hold_location_quietly`] saw was raised.
pub fn last_location() -> Option<String> {
    LOCATION.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// A joined thread's result, its panic going on up rather than reading as an
/// answer.
pub fn raise<T>(joined: std::thread::Result<T>) -> T {
    joined.unwrap_or_else(|panic| std::panic::resume_unwind(panic))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_keeps_this_crates_path_and_only_the_name_of_an_absolute_one() {
        assert_eq!(reported("src/web_audit/pool.rs"), "src/web_audit/pool.rs");
        let home = std::env::temp_dir()
            .join("registry")
            .join("flate2")
            .join("gz.rs");
        assert_eq!(reported(&home.to_string_lossy()), "gz.rs");
    }
}
