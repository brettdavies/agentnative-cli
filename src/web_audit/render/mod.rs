//! The `anc web` report: the exit table, the text rendering that leads with
//! the verdict and the failing rows with their fixes, the one-line `--check`
//! form, the stderr progress narration, and the failures a run ends with
//! when it has no scorecard. JSON mode prints the scorecard itself and lives
//! with the scorecard mirror.

mod exit;
mod failure;
mod lanes;
mod progress;
mod rows;
mod text;

pub use exit::{
    EXIT_CLEAN, EXIT_COULD_NOT_CHECK, EXIT_FAILURES, EXIT_TABLE, EXIT_WARNINGS, exit_code,
    exit_reason,
};
pub use failure::{
    Failure, ca_cert_failure, format_failure, insecure_scheme_failure, invalid_target_failure,
    unknown_check_failure, unreachable_failure, untrusted_certificate_failure,
};
pub use progress::{StderrProgress, progress_enabled};
pub use rows::prefix_for;
pub use text::{RenderOptions, advisory_note, format_check, format_text, non_comparable_note};

#[cfg(test)]
mod test_rows {
    use crate::web_audit::registry::{WebCheckKeyword, WebCheckTier};
    use crate::web_audit::scorecard::{Layer, ResultRow, ScorecardStatus};

    /// A scorecard row for the renderer's unit tests.
    pub(super) fn row(id: &str, keyword: WebCheckKeyword, status: ScorecardStatus) -> ResultRow {
        ResultRow {
            id: id.to_string(),
            label: format!("Row {id}"),
            category: "api".to_string(),
            group: "api".to_string(),
            layer: Layer::Web,
            keyword,
            tier: WebCheckTier::Required,
            principle: "P1".to_string(),
            status,
            na_reason: None,
            unprobed: false,
            advisory: None,
            evidence: Some(format!("evidence for {id}")),
            hosts: Vec::new(),
            host: None,
        }
    }
}
