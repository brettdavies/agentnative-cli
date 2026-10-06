//! Audit: `p5-must-force-yes`.
//!
//! Destructive operations (delete, overwrite, bulk modify) MUST require an
//! explicit `--force` or `--yes` flag — agents shouldn't be able to delete
//! resources by guessing the call shape, and the explicit flag makes the
//! intent auditable in process tables and shell history.
//!
//! Rubric: identify destructive subcommands via [`destructive_subcommands`],
//! probe each one's `--help`, and audit for one of [`CONFIRM_FLAGS`] or a
//! flag the `.anc.toml` chain declares in `[p5] confirm_flags`. Fail when
//! any destructive subcommand lists none. Vacuous Skip when the binary has
//! no destructive subcommands.

use crate::anc_toml::{CONFIRM_FLAGS_KEY, Sourced};
use crate::audit::Audit;
use crate::audits::behavioral::destructive_ops::destructive_subcommands;
use crate::audits::behavioral::subcommand_help::probe_subcommands;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{
    AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence, Mitigation, Verdict,
};

/// Flags that confirm a destructive operation non-interactively.
const CONFIRM_FLAGS: &[&str] = &[
    "--force",
    "--yes",
    "-y",
    "-f",
    "--auto-approve",
    "--assume-yes",
    "--confirm",
];

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
        let declared_flags = project
            .anc_config
            .config()
            .map_or(&[][..], |cfg| cfg.p5.confirm_flags.as_slice());
        let verdict = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()).into(),
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
                    .into()
                } else {
                    let runner = project.runner_ref();
                    let subhelp = probe_subcommands(runner, top_help);
                    audit_force_yes(&destructive, &subhelp, declared_flags)
                }
            }
        };
        let status = match project.anc_config.void_note() {
            Some(note) => verdict.status.with_note(&note),
            None => verdict.status,
        };
        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
            mitigation: verdict.mitigation,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

/// Pass when every destructive subcommand's `--help` lists a built-in
/// confirmation flag or one of `declared_flags`. A built-in match takes
/// priority; a Pass that needed a declared flag names the subcommand, the
/// flag, and the file that declared it.
pub(crate) fn audit_force_yes(
    destructive: &[String],
    subhelp: &[(String, HelpOutput)],
    declared_flags: &[Sourced<String>],
) -> Verdict {
    let mut missing: Vec<&str> = Vec::new();
    let mut confirmed_by_declaration: Vec<String> = Vec::new();
    for verb in destructive {
        let Some((_, help)) = subhelp.iter().find(|(name, _)| name == verb) else {
            missing.push(verb.as_str());
            continue;
        };
        if CONFIRM_FLAGS.iter().any(|flag| help.advertises_flag(flag)) {
            continue;
        }
        match declared_flags
            .iter()
            .find(|flag| help.advertises_flag(&flag.value))
        {
            Some(flag) => confirmed_by_declaration.push(format!(
                "{verb} accepts {} via {}",
                flag.value,
                flag.cite(CONFIRM_FLAGS_KEY)
            )),
            None => missing.push(verb.as_str()),
        }
    }
    if missing.is_empty() {
        return Verdict {
            status: AuditStatus::Pass,
            mitigation: (!confirmed_by_declaration.is_empty())
                .then(|| Mitigation::Config(confirmed_by_declaration.join("; "))),
        };
    }
    AuditStatus::Fail(format!(
        "destructive subcommand(s) whose --help lists no confirmation flag: {}. \
         Accepted flags: {}. Irreversible operations must require explicit \
         confirmation so they can't be invoked accidentally.",
        missing.join(", "),
        accepted_flags(declared_flags),
    ))
    .into()
}

/// The built-in names, then each declared flag with the file it came from.
fn accepted_flags(declared_flags: &[Sourced<String>]) -> String {
    CONFIRM_FLAGS
        .iter()
        .map(|flag| (*flag).to_string())
        .chain(
            declared_flags
                .iter()
                .map(|flag| format!("{} via {}", flag.value, flag.cite(CONFIRM_FLAGS_KEY))),
        )
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hp(raw: &str) -> HelpOutput {
        HelpOutput::from_raw(raw)
    }

    fn declared(flag: &str, file: &str) -> Sourced<String> {
        Sourced {
            value: flag.to_string(),
            file: file.to_string(),
        }
    }

    const GO_STYLE_DESTROY_HELP: &str = "Usage: tool destroy [options]\n\nOptions:\n\n  -auto-approve          Skip interactive approval.\n\n  -lock=false            Don't hold a lock.\n";

    #[test]
    fn a_declared_flag_confirms_where_the_builtins_do_not() {
        let subhelp = vec![("destroy".to_string(), hp(GO_STYLE_DESTROY_HELP))];
        let destructive = ["destroy".to_string()];

        assert!(matches!(
            audit_force_yes(&destructive, &subhelp, &[]).status,
            AuditStatus::Fail(_)
        ));

        let verdict = audit_force_yes(
            &destructive,
            &subhelp,
            &[declared("-auto-approve", ".anc.toml")],
        );
        assert_eq!(verdict.status, AuditStatus::Pass);
        assert_eq!(
            verdict.mitigation,
            Some(Mitigation::Config(
                "destroy accepts -auto-approve via .anc.toml [p5].confirm_flags".into()
            ))
        );
    }

    #[test]
    fn a_builtin_flag_takes_priority_over_a_declared_one() {
        let subhelp = vec![
            (
                "delete".to_string(),
                hp("Options:\n  -auto-approve  Skip approval.\n  --force        Skip approval.\n"),
            ),
            ("destroy".to_string(), hp(GO_STYLE_DESTROY_HELP)),
        ];
        let destructive = ["delete".to_string(), "destroy".to_string()];

        let verdict = audit_force_yes(
            &destructive,
            &subhelp,
            &[declared("-auto-approve", "~/.anc.toml")],
        );

        assert_eq!(verdict.status, AuditStatus::Pass);
        assert_eq!(
            verdict.mitigation,
            Some(Mitigation::Config(
                "destroy accepts -auto-approve via ~/.anc.toml [p5].confirm_flags".into()
            ))
        );
    }

    #[test]
    fn a_declared_flag_missing_from_the_subcommand_help_still_fails_and_is_named() {
        let subhelp = vec![(
            "destroy".to_string(),
            hp("Usage: tool destroy [options]\n\n  Destroy everything.\n"),
        )];

        match audit_force_yes(
            &["destroy".to_string()],
            &subhelp,
            &[declared("-auto-approve", ".anc.toml")],
        )
        .status
        {
            AuditStatus::Fail(msg) => assert!(
                msg.contains("--confirm, -auto-approve via .anc.toml [p5].confirm_flags."),
                "{msg}"
            ),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn a_void_config_says_no_setting_applied() {
        let mut project = crate::audits::behavioral::tests::test_project_with_sh_script(
            r#"case "$*" in
  "delete --help") printf 'Usage: tool delete <ID>\n\nOptions:\n  -h, --help  Show help.\n' ;;
  *) printf 'Usage: tool <COMMAND>\n\nCommands:\n  delete  Delete an item\n' ;;
esac"#,
        );
        project.anc_config.load = crate::anc_toml::AncConfigLoad::Invalid(
            "could not parse .anc.toml at .anc.toml: bad".into(),
        );

        match ForceYesAudit.run(&project).expect("audit runs").status {
            AuditStatus::Fail(msg) => assert!(
                msg.ends_with(
                    "No .anc.toml setting applied: could not parse .anc.toml at .anc.toml: bad."
                ),
                "{msg}"
            ),
            other => panic!("expected Fail, got {other:?}"),
        }
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
            audit_force_yes(&["delete".to_string()], &subhelp, &[]).status,
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
            audit_force_yes(&["purge".to_string()], &subhelp, &[]).status,
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_on_widely_used_confirmation_flags() {
        for flag in ["--auto-approve", "--assume-yes", "--confirm"] {
            let subhelp = vec![(
                "destroy".to_string(),
                hp(&format!(
                    "Usage: tool destroy\n\nOptions:\n      {flag}    Skip the prompt.\n  -h, --help    Show help.\n"
                )),
            )];
            assert_eq!(
                audit_force_yes(&["destroy".to_string()], &subhelp, &[]).status,
                AuditStatus::Pass,
                "{flag} confirms a destructive subcommand"
            );
        }
    }

    #[test]
    fn fail_names_the_accepted_flags() {
        let subhelp = vec![(
            "delete".to_string(),
            hp("Usage: tool delete <ID>\n\nOptions:\n  -h, --help    Show help.\n"),
        )];
        match audit_force_yes(&["delete".to_string()], &subhelp, &[]).status {
            AuditStatus::Fail(msg) => assert!(
                msg.contains("Accepted flags: --force, --yes, -y, -f, --auto-approve, --assume-yes, --confirm."),
                "{msg}"
            ),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn fail_when_destructive_subcommand_missing_flags() {
        let subhelp = vec![(
            "delete".to_string(),
            hp("Usage: tool delete <ID>\n\nOptions:\n  -h, --help    Show help.\n"),
        )];
        match audit_force_yes(&["delete".to_string()], &subhelp, &[]).status {
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
        match audit_force_yes(&["delete".to_string()], &subhelp, &[]).status {
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
        assert_eq!(
            audit_force_yes(&destructive, &subhelp, &[]).status,
            AuditStatus::Pass
        );
    }
}
