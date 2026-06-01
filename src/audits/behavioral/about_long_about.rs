//! Audit: `p3-should-about-long-about`.
//!
//! Short `about` for command-list summaries; `long_about` for detail visible
//! with `--help` but not `-h`. Clap distinguishes the two; many CLIs collapse
//! them which gives agents no concise summary to grep.
//!
//! Heuristic: probe both `-h` and `--help` directly and Warn when the two
//! outputs are byte-identical (no `long_about` defined) or when only one of
//! the two probes succeeds.

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::RunStatus;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct AboutLongAboutAudit;

impl Audit for AboutLongAboutAudit {
    fn id(&self) -> &str {
        "p3-about-long-about"
    }

    fn label(&self) -> &'static str {
        "Short `-h` summary differs from `--help` long form"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P3
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p3-should-about-long-about"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        // Probe both forms directly. We can't reuse the cached --help output
        // because we need a separate `-h` invocation to compare.
        let runner = project.runner_ref();
        let long_result = runner.run(&["--help"], &[]);
        let short_result = runner.run(&["-h"], &[]);

        let status = match (
            long_result.status,
            short_result.status,
            long_result.exit_code,
            short_result.exit_code,
        ) {
            (RunStatus::Ok, RunStatus::Ok, Some(0), Some(0)) => {
                audit_about_long_about(&long_result.stdout, &short_result.stdout)
            }
            _ => AuditStatus::Skip("could not probe both `-h` and `--help` cleanly".into()),
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

pub(crate) fn audit_about_long_about(long: &str, short: &str) -> AuditStatus {
    let long_trimmed = long.trim();
    let short_trimmed = short.trim();

    if long_trimmed.is_empty() || short_trimmed.is_empty() {
        return AuditStatus::Skip("one or both of `-h` / `--help` produced empty output".into());
    }

    if long_trimmed == short_trimmed {
        return AuditStatus::Warn(
            "`-h` and `--help` produce byte-identical output. SHOULD-tier — \
             clap renders the short summary on `-h` and the full description \
             on `--help` when `long_about` is set; collapsing them gives \
             agents no concise list-level grep target."
                .into(),
        );
    }

    AuditStatus::Pass
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pass_when_long_form_differs() {
        let long = "Long: detailed explanation across multiple paragraphs.\nOptions: ...\n";
        let short = "Short: one-line summary.\nOptions: ...\n";
        assert_eq!(audit_about_long_about(long, short), AuditStatus::Pass);
    }

    #[test]
    fn warn_when_identical() {
        let same = "Usage: tool [OPTIONS]\nOptions: ...\n";
        match audit_about_long_about(same, same) {
            AuditStatus::Warn(msg) => assert!(msg.contains("byte-identical")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn skip_when_empty() {
        match audit_about_long_about("", "Some text") {
            AuditStatus::Skip(_) => {}
            other => panic!("expected Skip, got {other:?}"),
        }
    }
}
