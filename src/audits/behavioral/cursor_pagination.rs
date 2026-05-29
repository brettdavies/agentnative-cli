//! Audit: `p7-may-cursor-pagination`.
//!
//! Cursor-based pagination flags (`--after`, `--before`, `--cursor`, `--page`)
//! for efficient traversal of large result sets. MAY-tier; applicability
//! gated on the presence of a list-style subcommand.

use crate::audit::Audit;
use crate::audits::behavioral::list_style::has_list_style_subcommand;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

const CURSOR_FLAGS: &[&str] = &["--after", "--before", "--cursor", "--page", "--offset"];

pub struct CursorPaginationAudit;

impl Audit for CursorPaginationAudit {
    fn id(&self) -> &str {
        "p7-cursor-pagination"
    }

    fn label(&self) -> &'static str {
        "Cursor-based pagination flags for list traversal"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P7
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p7-may-cursor-pagination"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_cursor_pagination(help),
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

pub(crate) fn audit_cursor_pagination(help: &HelpOutput) -> AuditStatus {
    if !has_list_style_subcommand(help) {
        return AuditStatus::Skip(
            "no list-style subcommand detected; vacuous skip for the list-only MAY.".into(),
        );
    }

    let has_cursor = help
        .flags()
        .iter()
        .any(|f| CURSOR_FLAGS.iter().any(|name| f.matches(name)));

    if has_cursor {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(format!(
            "list-style subcommand present but no cursor/page flag advertised \
             (looked for {}). MAY-tier — cursor pagination lets agents \
             traverse large result sets without re-scanning earlier pages.",
            CURSOR_FLAGS.join(", "),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_when_no_list_subcommand() {
        let help =
            HelpOutput::from_raw("Usage: tool [COMMAND]\n\nCommands:\n  audit    Run audits.\n");
        match audit_cursor_pagination(&help) {
            AuditStatus::Skip(msg) => assert!(msg.contains("vacuous")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn pass_with_cursor_flag() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  list     List items.\n\n\
             Options:\n      --cursor <C>    Pagination cursor.\n  -h, --help\n",
        );
        assert_eq!(audit_cursor_pagination(&help), AuditStatus::Pass);
    }

    #[test]
    fn pass_with_after_before_flags() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  search   Search items.\n\n\
             Options:\n      --after <ID>    Start after.\n      --before <ID>    End before.\n",
        );
        assert_eq!(audit_cursor_pagination(&help), AuditStatus::Pass);
    }

    #[test]
    fn warn_when_list_without_cursor() {
        let help = HelpOutput::from_raw(
            "Usage: tool [COMMAND]\n\nCommands:\n  list     List items.\n\n\
             Options:\n  -h, --help    Show help.\n",
        );
        match audit_cursor_pagination(&help) {
            AuditStatus::Warn(msg) => assert!(msg.contains("cursor")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
