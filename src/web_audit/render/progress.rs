//! The stderr narration a run prints on a terminal.

use std::io::IsTerminal;

use crate::web_audit::engine::ProgressSink;
use crate::web_audit::registry::CHECKS;
use crate::web_audit::scorecard::EngineResult;

/// Whether a run narrates its progress: only on a terminal, never when
/// quieted and never when `NO_COLOR` asks for a plain stream. A piped or
/// redirected run stays silent, so a twenty-five second audit reads as
/// work in progress to a person and as nothing at all to a program.
pub fn progress_enabled(quiet: bool) -> bool {
    !quiet
        && std::io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

/// Narrates a run on stderr: the discovered endpoint, then a counter that
/// names the check it just finished.
#[derive(Debug)]
pub struct StderrProgress {
    done: usize,
    total: usize,
}

impl StderrProgress {
    /// A sink counting toward the registry's check total.
    pub fn new() -> Self {
        StderrProgress {
            done: 0,
            total: CHECKS.len(),
        }
    }
}

impl Default for StderrProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressSink for StderrProgress {
    fn discovery(&mut self, endpoint: Option<&str>) {
        match endpoint {
            Some(endpoint) => eprintln!("  mcp endpoint: {endpoint}"),
            None => eprintln!("  mcp endpoint: none discovered"),
        }
    }

    fn result(&mut self, result: &EngineResult) {
        self.done += 1;
        eprint!("\r  {}/{} {:<40}", self.done, self.total, result.check.id);
        if self.done == self.total {
            eprintln!();
        }
    }
}
