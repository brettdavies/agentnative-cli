//! Check: `p6-must-sigterm` (Python).
//!
//! Mirrors the Rust counterpart. Detects `signal.signal(signal.SIGTERM, ...)`,
//! `loop.add_signal_handler(signal.SIGTERM, ...)`, and equivalent asyncio
//! patterns. Applicability is gated by the same long-running-operation
//! heuristic as Rust.

use crate::check::Check;
use crate::project::{Language, Project};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

const SIGTERM_HANDLER_SIGNALS: &[&str] = &[
    "signal.signal(signal.SIGTERM",
    "signal.signal(SIGTERM",
    "add_signal_handler(signal.SIGTERM",
    "add_signal_handler(SIGTERM",
    "signal.SIGTERM",
    // Frameworks that wrap the underlying call:
    "graceful_shutdown",
    "@on_shutdown",
];

const LONG_RUNNING_SIGNALS: &[&str] = &[
    "def serve",
    "def daemon",
    "def watch",
    "def tail",
    "asyncio.run",
    "uvicorn.run",
    "flask_app.run",
    "while True:",
    "FastAPI(",
    "Flask(",
];

pub struct SigtermPythonCheck;

impl Check for SigtermPythonCheck {
    fn id(&self) -> &str {
        "p6-sigterm"
    }

    fn label(&self) -> &'static str {
        "Long-running CLI handles SIGTERM"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P6
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-must-sigterm"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Python)
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let parsed = project.parsed_files();
        let mut has_handler = false;
        let mut has_long_running = false;

        for (_path, parsed_file) in parsed.iter() {
            let src = &parsed_file.source;
            if !has_handler && SIGTERM_HANDLER_SIGNALS.iter().any(|sig| src.contains(sig)) {
                has_handler = true;
            }
            if !has_long_running && LONG_RUNNING_SIGNALS.iter().any(|sig| src.contains(sig)) {
                has_long_running = true;
            }
            if has_handler && has_long_running {
                break;
            }
        }

        let status = match (has_long_running, has_handler) {
            (false, _) => CheckStatus::Pass,
            (true, true) => CheckStatus::Pass,
            (true, false) => CheckStatus::Fail(
                "long-running operation detected (server/daemon/asyncio.run \
                 marker present) but no SIGTERM handler found. Install one \
                 via signal.signal(signal.SIGTERM, ...) or asyncio's \
                 add_signal_handler to release locks and flush state on \
                 shutdown."
                    .into(),
            ),
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
        })
    }
}

pub(crate) fn check_sigterm_python(source: &str) -> CheckStatus {
    let has_handler = SIGTERM_HANDLER_SIGNALS
        .iter()
        .any(|sig| source.contains(sig));
    let has_long_running = LONG_RUNNING_SIGNALS.iter().any(|sig| source.contains(sig));

    match (has_long_running, has_handler) {
        (false, _) => CheckStatus::Pass,
        (true, true) => CheckStatus::Pass,
        (true, false) => {
            CheckStatus::Fail("long-running operation detected but no SIGTERM handler found".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_signal_signal() {
        let source = r#"
import signal

def serve():
    signal.signal(signal.SIGTERM, lambda *_: shutdown())

def shutdown():
    pass
"#;
        assert_eq!(check_sigterm_python(source), CheckStatus::Pass);
    }

    #[test]
    fn happy_path_asyncio_add_signal_handler() {
        let source = r#"
import asyncio
import signal

async def serve():
    loop = asyncio.get_event_loop()
    loop.add_signal_handler(signal.SIGTERM, shutdown)

asyncio.run(serve())
"#;
        assert_eq!(check_sigterm_python(source), CheckStatus::Pass);
    }

    #[test]
    fn vacuous_pass_short_running() {
        let source = r#"
def main():
    print("hello")
"#;
        assert_eq!(check_sigterm_python(source), CheckStatus::Pass);
    }

    #[test]
    fn fail_long_running_no_handler() {
        let source = r#"
import asyncio

async def serve():
    while True:
        await asyncio.sleep(1)

asyncio.run(serve())
"#;
        match check_sigterm_python(source) {
            CheckStatus::Fail(msg) => assert!(msg.contains("SIGTERM")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }
}
