//! Audit: `p5-must-force-yes`.
//!
//! Destructive operations (delete, overwrite, bulk modify) MUST require an
//! explicit `--force` or `--yes` flag — agents shouldn't be able to delete
//! resources by guessing the call shape, and the explicit flag makes the
//! intent auditable in process tables and shell history.
//!
//! Rubric: identify destructive subcommands via [`destructive_subcommands`],
//! probe each one's `--help`, and audit for the presence of `--force`,
//! `--yes`, `-y`, or `-f`. Fail when any destructive subcommand lacks both.
//! Vacuous Skip when the binary has no destructive subcommands.

use crate::audit::Audit;
use crate::audits::behavioral::destructive_ops::destructive_subcommands;
use crate::audits::behavioral::subcommand_help::probe_subcommands;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

const REQUIRED_FLAGS: &[&str] = &["--force", "--yes", "-y", "-f"];

pub struct ForceYesAudit;

impl Audit for ForceYesAudit {
    fn id(&self) -> &str {
        "p5-force-yes"
    }

    fn label(&self) -> &'static str {
        "Destructive subcommands require `--force` or `--yes`"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P5
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p5-must-force-yes"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(top_help) => {
                let destructive: Vec<String> = destructive_subcommands(top_help)
                    .into_iter()
                    .cloned()
                    .collect();
                if destructive.is_empty() {
                    AuditStatus::Skip(
                        "no destructive subcommands detected; MUST applies conditionally to CLIs \
                         with destructive operations."
                            .into(),
                    )
                } else {
                    let runner = project.runner_ref();
                    let subhelp = probe_subcommands(runner, top_help);
                    audit_force_yes(&destructive, &subhelp)
                }
            }
        };
        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
            mitigation: None,
        })
    }
}

pub(crate) fn audit_force_yes(
    destructive: &[String],
    subhelp: &[(String, HelpOutput)],
) -> AuditStatus {
    let mut missing: Vec<&str> = Vec::new();
    for verb in destructive {
        let entry = subhelp.iter().find(|(name, _)| name == verb);
        match entry {
            Some((_, help)) if has_force_or_yes(help) => {}
            Some(_) => missing.push(verb.as_str()),
            None => missing.push(verb.as_str()),
        }
    }
    if missing.is_empty() {
        AuditStatus::Pass
    } else {
        AuditStatus::Fail(format!(
            "destructive subcommand(s) without `--force` or `--yes`: {}. \
             Irreversible operations must require explicit confirmation so \
             they can't be invoked accidentally.",
            missing.join(", ")
        ))
    }
}

fn has_force_or_yes(help: &HelpOutput) -> bool {
    help.flags()
        .iter()
        .any(|f| REQUIRED_FLAGS.iter().any(|name| f.matches(name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hp(raw: &str) -> HelpOutput {
        HelpOutput::from_raw(raw)
    }

    #[test]
    fn pass_when_destructive_subcommand_has_force() {
        let subhelp = vec![(
            "delete".to_string(),
            hp(
                "Usage: tool delete [OPTIONS] <ID>\n\nOptions:\n      --force    Skip confirmation.\n  -h, --help    Show help.\n",
            ),
        )];
        assert_eq!(
            audit_force_yes(&["delete".to_string()], &subhelp),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_when_destructive_subcommand_has_yes() {
        let subhelp = vec![(
            "purge".to_string(),
            hp(
                "Usage: tool purge\n\nOptions:\n  -y, --yes    Confirm purge.\n  -h, --help    Show help.\n",
            ),
        )];
        assert_eq!(
            audit_force_yes(&["purge".to_string()], &subhelp),
            AuditStatus::Pass
        );
    }

    #[test]
    fn fail_when_destructive_subcommand_missing_flags() {
        let subhelp = vec![(
            "delete".to_string(),
            hp("Usage: tool delete <ID>\n\nOptions:\n  -h, --help    Show help.\n"),
        )];
        match audit_force_yes(&["delete".to_string()], &subhelp) {
            AuditStatus::Fail(msg) => {
                assert!(msg.contains("delete"));
                assert!(msg.contains("--force"));
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn fail_when_destructive_subcommand_help_missing() {
        // Subcommand listed as destructive but the probe returned nothing
        // (timeout, crash, refused --help). Treat as Fail; the operator must
        // surface a confirmation flag in the documented help text.
        let subhelp: Vec<(String, HelpOutput)> = Vec::new();
        match audit_force_yes(&["delete".to_string()], &subhelp) {
            AuditStatus::Fail(msg) => assert!(msg.contains("delete")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn pass_with_mixed_destructive_set() {
        let subhelp = vec![
            (
                "delete".to_string(),
                hp("Options:\n  --force\n  -h, --help\n"),
            ),
            (
                "purge".to_string(),
                hp("Options:\n  -y, --yes\n  -h, --help\n"),
            ),
        ];
        let destructive = vec!["delete".to_string(), "purge".to_string()];
        assert_eq!(audit_force_yes(&destructive, &subhelp), AuditStatus::Pass);
    }
}
