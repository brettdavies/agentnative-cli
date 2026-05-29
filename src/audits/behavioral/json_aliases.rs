//! Audit: `p2-should-json-aliases`.
//!
//! `--json` and `--jsonl` are accepted as short-form aliases for
//! `--output json` and `--output jsonl`. The short forms work alongside the
//! canonical enum so agents and pipelines can use either spelling.
//!
//! Universal applicability. Pass when at least one of `--json` / `--jsonl`
//! is advertised. Warn when neither short form is present (the SHOULD is not
//! met). Skip when the help cannot be probed.

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct JsonAliasesAudit;

impl Audit for JsonAliasesAudit {
    fn id(&self) -> &str {
        "p2-json-aliases"
    }

    fn label(&self) -> &'static str {
        "--json / --jsonl short aliases for --output"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P2
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p2-should-json-aliases"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_json_aliases(help),
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

/// Core unit for tests. Pass when either alias is present; Warn otherwise.
pub(crate) fn audit_json_aliases(help: &HelpOutput) -> AuditStatus {
    let has_json = help.flags().iter().any(|f| f.matches("--json"));
    let has_jsonl = help.flags().iter().any(|f| f.matches("--jsonl"));

    if has_json || has_jsonl {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(
            "no --json or --jsonl short alias found. Agents and pipelines \
             benefit from short forms alongside the canonical `--output` enum."
                .into(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_json_alias() {
        let help = HelpOutput::from_raw(
            "Options:\n      --json    Emit JSON.\n  -h, --help    Show help.\n",
        );
        assert_eq!(audit_json_aliases(&help), AuditStatus::Pass);
    }

    #[test]
    fn happy_path_jsonl_alias() {
        let help = HelpOutput::from_raw(
            "Options:\n      --jsonl   Emit JSONL.\n  -h, --help    Show help.\n",
        );
        assert_eq!(audit_json_aliases(&help), AuditStatus::Pass);
    }

    #[test]
    fn warn_no_alias() {
        let help = HelpOutput::from_raw(
            "Options:\n      --output <FORMAT>   Output format.\n  -h, --help    Show help.\n",
        );
        match audit_json_aliases(&help) {
            AuditStatus::Warn(msg) => assert!(msg.contains("--json")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
