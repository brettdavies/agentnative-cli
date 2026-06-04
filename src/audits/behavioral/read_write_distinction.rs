//! Audit: `p5-must-read-write-distinction`.
//!
//! The distinction between read and write commands MUST be clear from the
//! command name and help text alone — an agent shouldn't need to consult
//! external documentation to know whether a call is safe to retry.
//!
//! Behavioral inference: a CLI satisfies the distinction when it ships BOTH
//! read-pattern subcommands (`list`, `get`, `show`, `query`, `find`,
//! `search`) AND write-pattern subcommands (destructive verbs plus `create`,
//! `add`, `update`, `set`, `put`). The presence of both surfaces is the
//! observable signature of a CLI that separates the two intents.
//!
//! - Pass when both surfaces are present.
//! - Warn when only one side appears — the CLI may be all-read or all-write,
//!   in which case the distinction is unobservable but not necessarily
//!   missing.
//! - Skip when neither side appears (no recognizable verb subcommands).

use crate::audit::Audit;
use crate::audits::behavioral::destructive_ops::{is_read_verb, is_write_verb};
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct ReadWriteDistinctionAudit;

impl Audit for ReadWriteDistinctionAudit {
    fn id(&self) -> &str {
        "p5-read-write-distinction"
    }

    fn label(&self) -> &'static str {
        "Read and write surfaces are both visible in subcommand list"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P5
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p5-must-read-write-distinction"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_read_write(help),
        };
        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
            mitigation: None,
        })
    }
}

pub(crate) fn audit_read_write(help: &HelpOutput) -> AuditStatus {
    let mut reads: Vec<&str> = Vec::new();
    let mut writes: Vec<&str> = Vec::new();
    for name in help.subcommands() {
        if is_read_verb(name) {
            reads.push(name.as_str());
        }
        if is_write_verb(name) {
            writes.push(name.as_str());
        }
    }

    match (reads.is_empty(), writes.is_empty()) {
        (false, false) => AuditStatus::Pass,
        (true, true) => AuditStatus::Skip(
            "no recognizable read or write subcommand verbs; the read/write \
             distinction is unobservable from the help surface alone."
                .into(),
        ),
        (false, true) => AuditStatus::Warn(format!(
            "read-pattern subcommand(s) present ({}) but no write-pattern \
             surface detected. If the CLI is read-only by design the MUST is \
             satisfied vacuously; otherwise the write surface needs an \
             agent-recognizable verb (create/add/update/set/delete/…).",
            reads.join(", ")
        )),
        (true, false) => AuditStatus::Warn(format!(
            "write-pattern subcommand(s) present ({}) but no read-pattern \
             surface detected. If the CLI is write-only by design the MUST \
             is satisfied vacuously; otherwise expose the read surface with \
             agent-recognizable verbs (list/get/show/query/find/search).",
            writes.join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hp_with_commands(commands: &[(&str, &str)]) -> HelpOutput {
        let mut raw = String::from("Usage: tool [COMMAND]\n\nCommands:\n");
        for (name, desc) in commands {
            raw.push_str(&format!("  {name:10}  {desc}\n"));
        }
        HelpOutput::from_raw(raw)
    }

    #[test]
    fn pass_when_both_surfaces_present() {
        let help = hp_with_commands(&[
            ("list", "List items."),
            ("create", "Create item."),
            ("delete", "Delete item."),
        ]);
        assert_eq!(audit_read_write(&help), AuditStatus::Pass);
    }

    #[test]
    fn pass_with_minimal_read_and_write() {
        let help = hp_with_commands(&[("get", "Get an item."), ("set", "Set an item.")]);
        assert_eq!(audit_read_write(&help), AuditStatus::Pass);
    }

    #[test]
    fn warn_when_only_read_surface() {
        let help = hp_with_commands(&[("list", "List items."), ("show", "Show item.")]);
        match audit_read_write(&help) {
            AuditStatus::Warn(msg) => {
                assert!(msg.contains("read-pattern"));
                assert!(msg.contains("no write-pattern"));
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn warn_when_only_write_surface() {
        let help = hp_with_commands(&[("create", "Create item."), ("delete", "Delete item.")]);
        match audit_read_write(&help) {
            AuditStatus::Warn(msg) => {
                assert!(msg.contains("write-pattern"));
                assert!(msg.contains("no read-pattern"));
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn skip_when_neither_surface() {
        let help = hp_with_commands(&[("build", "Build it."), ("audit", "Run audits.")]);
        match audit_read_write(&help) {
            AuditStatus::Skip(_) => {}
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn skip_when_no_subcommands() {
        let help = HelpOutput::from_raw("Usage: tool [OPTIONS]\n\nOptions:\n  -h, --help\n");
        match audit_read_write(&help) {
            AuditStatus::Skip(_) => {}
            other => panic!("expected Skip, got {other:?}"),
        }
    }
}
