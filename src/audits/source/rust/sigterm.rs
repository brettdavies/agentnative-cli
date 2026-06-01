//! Audit: `p6-must-sigterm` (Rust).
//!
//! Long-running operations handle SIGTERM gracefully: flush or roll back
//! partial writes, release locks, exit non-zero within a bounded window. The
//! next invocation succeeds without manual cleanup.
//!
//! Detection (source-layer): scan for SIGTERM-handling primitives across the
//! common Rust signal-handling APIs — `signal_hook`, `tokio::signal::unix`'s
//! `SignalKind::terminate`, and direct `libc::SIGTERM` usage.
//!
//! Applicability gate: the requirement is conditional on "CLI has long-running
//! operations". The audit uses a heuristic on parsed file content — presence
//! of long-running subcommand names (`serve`, `daemon`, `watch`, `tail`,
//! `start`) or async runtime markers (`tokio::main`) — to decide whether to
//! demand SIGTERM handling. When no long-running signal is found, vacuous
//! Pass.

use crate::audit::Audit;
use crate::project::{Language, Project};
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Substrings whose presence anywhere in the source signals SIGTERM-handling
/// intent. Conservative — these are explicit handler installations, not just
/// SIGTERM mentions in comments.
const SIGTERM_HANDLER_SIGNALS: &[&str] = &[
    "signal_hook::flag::register",
    "signal_hook::iterator::Signals",
    "signal_hook::consts::SIGTERM",
    "SignalKind::terminate",
    "signal(SignalKind::terminate",
    "libc::SIGTERM",
];

/// Heuristic markers that the CLI runs long-running operations. Any hit
/// activates the SIGTERM requirement.
const LONG_RUNNING_SIGNALS: &[&str] = &[
    "fn serve",
    "fn daemon",
    "fn watch",
    "fn tail",
    "fn run_server",
    "tokio::main",
    "actix_web",
    "axum::Router",
    "warp::serve",
    "loop {",
    ".watch(",
    "watch_for_changes",
];

pub struct SigtermAudit;

impl Audit for SigtermAudit {
    fn id(&self) -> &str {
        "p6-sigterm"
    }

    fn label(&self) -> &'static str {
        "Long-running CLI handles SIGTERM"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P6
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Source
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p6-must-sigterm"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.language == Some(Language::Rust)
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
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
            (false, _) => AuditStatus::Pass, // vacuous — not long-running
            (true, true) => AuditStatus::Pass,
            (true, false) => AuditStatus::Fail(
                "long-running operation detected (server/daemon/watch/tail \
                 marker present) but no SIGTERM handler found. Install one \
                 via signal_hook or tokio::signal::unix::SignalKind::terminate \
                 to flush state and exit cleanly on shutdown."
                    .into(),
            ),
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
        })
    }
}

/// Core unit for tests. Returns Pass / Fail per the applicability + handler
/// matrix. Unit testable without a `Project`. The trait `run()` aggregates
/// across multiple parsed files (a server and its signal-installer can live in
/// different files); this helper exists for single-source-string testing.
#[cfg(test)]
pub(crate) fn audit_sigterm(source: &str) -> AuditStatus {
    let has_handler = SIGTERM_HANDLER_SIGNALS
        .iter()
        .any(|sig| source.contains(sig));
    let has_long_running = LONG_RUNNING_SIGNALS.iter().any(|sig| source.contains(sig));

    match (has_long_running, has_handler) {
        (false, _) => AuditStatus::Pass,
        (true, true) => AuditStatus::Pass,
        (true, false) => {
            AuditStatus::Fail("long-running operation detected but no SIGTERM handler found".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_signal_hook() {
        let source = r#"
use signal_hook::consts::SIGTERM;
use signal_hook::flag::register;

#[tokio::main]
async fn main() {
    let term = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    signal_hook::flag::register(SIGTERM, term.clone()).unwrap();
    serve().await;
}

async fn serve() {}
"#;
        assert_eq!(audit_sigterm(source), AuditStatus::Pass);
    }

    #[test]
    fn happy_path_tokio_signal_kind_terminate() {
        let source = r#"
use tokio::signal::unix::{signal, SignalKind};

#[tokio::main]
async fn main() {
    let mut term = signal(SignalKind::terminate()).unwrap();
    fn watch() {}
    tokio::select! {
        _ = term.recv() => {},
    }
}
"#;
        assert_eq!(audit_sigterm(source), AuditStatus::Pass);
    }

    #[test]
    fn vacuous_pass_short_running() {
        let source = r#"
fn main() {
    println!("hello");
}
"#;
        assert_eq!(audit_sigterm(source), AuditStatus::Pass);
    }

    #[test]
    fn fail_long_running_no_handler() {
        let source = r#"
use tokio::main;

#[tokio::main]
async fn main() {
    serve().await;
}

async fn serve() {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
"#;
        match audit_sigterm(source) {
            AuditStatus::Fail(msg) => assert!(msg.contains("SIGTERM")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }
}
