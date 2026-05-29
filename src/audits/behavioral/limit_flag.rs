//! Audit: `p7-should-limit`.
//!
//! A `--limit` or `--max-results` flag lets callers request exactly the
//! number of items they want from list-style commands. SHOULD-tier;
//! applicability is gated on the presence of a list-style subcommand
//! (see `list_style::has_list_style_subcommand`).

use crate::audit::Audit;
use crate::audits::behavioral::list_style::has_list_style_subcommand;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

const LIMIT_FLAGS: &[&str] = &["--limit", "--max-results", "--max", "--top", "-n"];

pub struct LimitFlagAudit;

impl Audit for LimitFlagAudit {
    fn id(&self) -> &str {
        "p7-limit"
    }

    fn label(&self) -> &'static str {
        "`--limit` / `--max-results` flag for list operations"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P7
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p7-should-limit"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_limit_flag(help),
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

pub(crate) fn audit_limit_flag(help: &HelpOutput) -> AuditStatus {
    if !has_list_style_subcommand(help) {
        return AuditStatus::Skip(
            "no list-style subcommand detected (list/ls/search/query/find/show/get); \
             vacuous skip for the list-only SHOULD."
                .into(),
        );
    }

    let has_limit = help
        .flags()
        .iter()
        .any(|f| LIMIT_FLAGS.iter().any(|name| f.matches(name)));

    if has_limit {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(format!(
            "list-style subcommand present but no limit flag advertised \
             (looked for {}). SHOULD-tier — callers should be able to bound \
             response size directly rather than scrape-then-truncate.",
            LIMIT_FLAGS.join(", "),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_when_no_list_subcommand() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  audit    Run audits.\n  build    Build.\n",
        );
        match audit_limit_flag(&help) {
            AuditStatus::Skip(msg) => assert!(msg.contains("vacuous")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn pass_when_list_and_limit_present() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  list     List items.\n\n\
             Options:\n      --limit <N>    Max items.\n  -h, --help    Show help.\n",
        );
        assert_eq!(audit_limit_flag(&help), AuditStatus::Pass);
    }

    #[test]
    fn warn_when_list_without_limit() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  list     List items.\n\n\
             Options:\n  -h, --help    Show help.\n",
        );
        match audit_limit_flag(&help) {
            AuditStatus::Warn(msg) => assert!(msg.contains("--limit")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
