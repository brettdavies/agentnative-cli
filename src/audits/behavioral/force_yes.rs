//! Audit: `p5-must-force-yes`.
//!
//! Destructive operations (delete, overwrite, bulk modify) MUST require an
//! explicit `--force` or `--yes` flag — agents shouldn't be able to delete
//! resources by guessing the call shape, and the explicit flag makes the
//! intent auditable in process tables and shell history.
//!
//! Rubric: identify destructive subcommands via [`destructive_subcommands`],
//! less any the `.anc.toml` chain declares in `[p5] not_destructive`, probe
//! each one's `--help`, and audit for one of [`CONFIRM_FLAGS`] or a flag the
//! chain declares in `[p5] confirm_flags`. Fail when any destructive
//! subcommand lists none. Vacuous Skip when the binary has no destructive
//! subcommands.

use std::borrow::Cow;

use crate::anc_toml::{CONFIRM_FLAGS_KEY, NOT_DESTRUCTIVE_KEY, Sourced};
use crate::audit::Audit;
use crate::audits::behavioral::destructive_ops::destructive_subcommands;
use crate::audits::behavioral::subcommand_help::{dash_rule_notes, probe_subcommands};
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
        let p5 = project.anc_config.config().map(|cfg| &cfg.p5);
        let declared_flags = p5.map_or(&[][..], |p5| p5.confirm_flags.as_slice());
        let not_destructive = p5.map_or(&[][..], |p5| p5.not_destructive.as_slice());
        let verdict = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()).into(),
            Some(top_help) => {
                let (destructive, excluded) =
                    without_declared(&destructive_subcommands(top_help), not_destructive);
                let verdict = if destructive.is_empty() {
                    let found = if excluded.is_empty() {
                        "no destructive subcommands detected"
                    } else {
                        "no destructive subcommands remain"
                    };
                    AuditStatus::Skip(format!(
                        "{found}; MUST applies conditionally to CLIs with destructive operations."
                    ))
                    .into()
                } else {
                    let runner = project.runner_ref();
                    let subhelp = probe_subcommands(runner, top_help);
                    audit_force_yes(&destructive, &subhelp, declared_flags)
                };
                with_exclusions(verdict, &excluded)
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

/// A declared flag as the lookup spells it. An entry written as a bare word
/// is a flag all the same: `noconfirm` is `--noconfirm`, and `y` is `-y`. An
/// entry that leads with `-`, `+` or `/` is a name as written.
fn as_flag_name(value: &str) -> Cow<'_, str> {
    if !value.starts_with(char::is_alphanumeric) {
        Cow::Borrowed(value)
    } else if value.chars().count() == 1 {
        Cow::Owned(format!("-{value}"))
    } else {
        Cow::Owned(format!("--{value}"))
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
        if help.find_flag(CONFIRM_FLAGS).is_some() {
            continue;
        }
        match declared_flags
            .iter()
            .find(|flag| help.find_flag(&[&as_flag_name(&flag.value)]).is_some())
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
    let declared_names: Vec<Cow<'_, str>> = declared_flags
        .iter()
        .map(|flag| as_flag_name(&flag.value))
        .collect();
    let wanted: Vec<&str> = CONFIRM_FLAGS
        .iter()
        .copied()
        .chain(declared_names.iter().map(AsRef::as_ref))
        .collect();
    AuditStatus::Fail(format!(
        "destructive subcommand(s) whose --help lists no confirmation flag: {}. \
         Accepted flags: {}. Irreversible operations must require explicit \
         confirmation so they can't be invoked accidentally.{}",
        missing.join(", "),
        accepted_flags(declared_flags),
        dash_rule_notes(&missing, subhelp, &wanted),
    ))
    .into()
}

/// Split `detected` into the subcommands that stay destructive and, cited
/// with the file that declared it, each one `not_destructive` removes.
pub(crate) fn without_declared(
    detected: &[&String],
    not_destructive: &[Sourced<String>],
) -> (Vec<String>, Vec<String>) {
    let mut destructive = Vec::new();
    let mut excluded = Vec::new();
    for name in detected {
        let lower = name.to_lowercase();
        match not_destructive.iter().find(|entry| entry.value == lower) {
            Some(entry) => excluded.push(format!("{name} via {}", entry.cite(NOT_DESTRUCTIVE_KEY))),
            None => destructive.push((*name).clone()),
        }
    }
    (destructive, excluded)
}

/// Name the subcommands a declaration removed: in a Pass's evidence beside
/// any other setting it needed, or at the end of any other status.
fn with_exclusions(verdict: Verdict, excluded: &[String]) -> Verdict {
    if excluded.is_empty() {
        return verdict;
    }
    let declared = format!("declared not destructive: {}", excluded.join(", "));
    let note = format!("Subcommands {declared}.");
    verdict.crediting(declared, &note)
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
    use crate::runner::help_probe::fixture_snapshots::fixture;

    fn hp(raw: &str) -> HelpOutput {
        HelpOutput::from_raw(raw)
    }

    fn declared(flag: &str, file: &str) -> Sourced<String> {
        Sourced {
            value: flag.to_string(),
            file: file.to_string(),
        }
    }

    // Modeled on pacman's `--noconfirm`, a confirmation flag outside the
    // built-in names.
    const NOCONFIRM_DESTROY_HELP: &str = "Usage: tool destroy [options]\n\nOptions:\n      --noconfirm        Do not ask for any confirmation.\n      --dbonly           Only modify database entries.\n";

    // terraform 1.16.4's `terraform force-unlock --help`.
    const TERRAFORM_FORCE_UNLOCK_HELP: &str = "Usage: terraform [global options] force-unlock LOCK_ID\n\n  Manually unlock the state for the defined configuration.\n\nOptions:\n\n  -force                 Don't ask for input for unlock confirmation.\n";

    #[test]
    fn a_declared_flag_confirms_where_the_builtins_do_not() {
        let subhelp = vec![("destroy".to_string(), hp(NOCONFIRM_DESTROY_HELP))];
        let destructive = ["destroy".to_string()];

        assert!(matches!(
            audit_force_yes(&destructive, &subhelp, &[]).status,
            AuditStatus::Fail(_)
        ));

        let verdict = audit_force_yes(
            &destructive,
            &subhelp,
            &[declared("--noconfirm", ".anc.toml")],
        );
        assert_eq!(verdict.status, AuditStatus::Pass);
        assert_eq!(
            verdict.mitigation,
            Some(Mitigation::Config(
                "destroy accepts --noconfirm via .anc.toml [p5].confirm_flags".into()
            ))
        );
    }

    #[test]
    fn a_builtin_flag_takes_priority_over_a_declared_one() {
        let subhelp = vec![
            (
                "delete".to_string(),
                hp("Options:\n  --noconfirm  Skip approval.\n  --force      Skip approval.\n"),
            ),
            ("destroy".to_string(), hp(NOCONFIRM_DESTROY_HELP)),
        ];
        let destructive = ["delete".to_string(), "destroy".to_string()];

        let verdict = audit_force_yes(
            &destructive,
            &subhelp,
            &[declared("--noconfirm", "~/.anc.toml")],
        );

        assert_eq!(verdict.status, AuditStatus::Pass);
        assert_eq!(
            verdict.mitigation,
            Some(Mitigation::Config(
                "destroy accepts --noconfirm via ~/.anc.toml [p5].confirm_flags".into()
            ))
        );
    }

    #[test]
    fn a_single_dash_word_confirms_in_a_help_without_double_dash_names() {
        let subhelp = vec![("force-unlock".to_string(), hp(TERRAFORM_FORCE_UNLOCK_HELP))];

        let verdict = audit_force_yes(&["force-unlock".to_string()], &subhelp, &[]);

        assert_eq!(verdict.status, AuditStatus::Pass);
        assert_eq!(verdict.mitigation, None);
    }

    #[test]
    fn a_single_dash_word_does_not_confirm_beside_double_dash_names() {
        let subhelp = vec![(
            "destroy".to_string(),
            hp("Options:\n  -force       Skip the prompt.\n  --help       Show help.\n"),
        )];

        match audit_force_yes(&["destroy".to_string()], &subhelp, &[]).status {
            AuditStatus::Fail(msg) => assert!(
                msg.ends_with(
                    "In `destroy`, `-force` is declared, but this help also declares \
                     double-dash names, so it does not count as `--force`."
                ),
                "{msg}"
            ),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn a_declared_double_dash_name_reaches_its_single_dash_spelling() {
        let subhelp = vec![(
            "destroy".to_string(),
            hp(
                "Usage: tool destroy [options]\n\nOptions:\n\n  -noconfirm             Skip interactive approval.\n\n  -lock=false            Don't hold a lock.\n",
            ),
        )];

        let verdict = audit_force_yes(
            &["destroy".to_string()],
            &subhelp,
            &[declared("--noconfirm", ".anc.toml")],
        );

        assert_eq!(verdict.status, AuditStatus::Pass);
        assert_eq!(
            verdict.mitigation,
            Some(Mitigation::Config(
                "destroy accepts --noconfirm via .anc.toml [p5].confirm_flags".into()
            ))
        );
    }

    fn confirms(help: &str, flag: &str) -> Verdict {
        let subhelp = vec![("destroy".to_string(), hp(help))];
        audit_force_yes(
            &["destroy".to_string()],
            &subhelp,
            &[declared(flag, ".anc.toml")],
        )
    }

    #[test]
    fn a_declared_single_dash_word_confirms_on_the_line_that_defines_it() {
        // terraform 1.16.4's `apply --help` declares no double-dash name, so
        // the built-in `--auto-approve` already answers.
        let apply = confirms(&fixture("terraform__apply_--help.txt"), "-auto-approve");
        assert_eq!(apply.status, AuditStatus::Pass);
        assert_eq!(apply.mitigation, None);

        // Beside a double-dash name the built-in does not answer, and the
        // declared spelling does.
        let mixed = confirms(
            "Options:\n  -auto-approve     Skip interactive approval.\n      --help        Show help.\n",
            "-auto-approve",
        );
        assert_eq!(mixed.status, AuditStatus::Pass);
        assert_eq!(
            mixed.mitigation,
            Some(Mitigation::Config(
                "destroy accepts -auto-approve via .anc.toml [p5].confirm_flags".into()
            ))
        );
    }

    #[test]
    fn a_declared_flag_named_only_in_a_wrapped_description_does_not_confirm() {
        let verdict = confirms(
            "Options:\n  --plan <FILE>     Apply the saved plan. This skips the prompt that\n                    -auto-approve skips on a fresh plan.\n      --help        Show help.\n",
            "-auto-approve",
        );
        assert!(
            matches!(verdict.status, AuditStatus::Fail(_)),
            "{:?}",
            verdict.status
        );
    }

    #[test]
    fn a_declared_flag_on_a_tab_indented_definition_line_confirms() {
        let verdict = confirms(
            "Usage of tool:\n\t-noconfirm\n\t\tSkip interactive approval\n\t--help\n\t\tShow help\n",
            "-noconfirm",
        );
        assert_eq!(verdict.status, AuditStatus::Pass);
        assert_eq!(
            verdict.mitigation,
            Some(Mitigation::Config(
                "destroy accepts -noconfirm via .anc.toml [p5].confirm_flags".into()
            ))
        );
    }

    #[test]
    fn a_declared_flag_written_without_dashes_is_a_flag() {
        let long = confirms(
            "Options:\n      --noconfirm    Do not ask.\n  -h, --help         Show help.\n",
            "noconfirm",
        );
        assert_eq!(long.status, AuditStatus::Pass);
        assert_eq!(
            long.mitigation,
            Some(Mitigation::Config(
                "destroy accepts noconfirm via .anc.toml [p5].confirm_flags".into()
            ))
        );

        let short = confirms(
            "Options:\n  -k             Do not ask.\n  -h, --help     Show help.\n",
            "k",
        );
        assert_eq!(short.status, AuditStatus::Pass);
    }

    #[test]
    fn a_declared_flag_that_leads_with_a_plus_is_matched_as_written() {
        let plus = confirms(
            "Options:\n  +n, --no-ask   Do not ask.\n  -h, --help     Show help.\n",
            "+n",
        );
        assert_eq!(plus.status, AuditStatus::Pass);
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
    fn a_declared_not_destructive_subcommand_leaves_the_destructive_set() {
        let detected = ["clean".to_string(), "Purge".to_string()];
        let detected: Vec<&String> = detected.iter().collect();

        let (destructive, excluded) =
            without_declared(&detected, &[declared("clean", ".anc.toml")]);

        assert_eq!(destructive, ["Purge"]);
        assert_eq!(excluded, ["clean via .anc.toml [p5].not_destructive"]);
    }

    /// A tool with `clean` (no confirmation flag) and `delete --force`.
    const CLEAN_AND_DELETE_CLI: &str = r#"case "$*" in
  "clean --help") printf 'Usage: tool clean\n\nOptions:\n  -h, --help  Show help.\n' ;;
  "delete --help") printf 'Usage: tool delete <ID>\n\nOptions:\n      --force  Skip the prompt.\n' ;;
  *) printf 'Usage: tool <COMMAND>\n\nCommands:\n  clean   Remove cached logs\n  delete  Delete an item\n' ;;
esac"#;

    fn run_with_not_destructive(script: &str, names: &[&str]) -> AuditResult {
        let mut project = crate::audits::behavioral::tests::test_project_with_sh_script(script);
        let mut cfg = crate::anc_toml::AncConfig::default();
        cfg.p5.not_destructive = names
            .iter()
            .map(|name| declared(name, ".anc.toml"))
            .collect();
        project.anc_config.load = crate::anc_toml::AncConfigLoad::Loaded(cfg);
        ForceYesAudit.run(&project).expect("audit runs")
    }

    #[test]
    fn declaring_the_unconfirmed_subcommand_not_destructive_passes_and_says_so() {
        let without = run_with_not_destructive(CLEAN_AND_DELETE_CLI, &[]);
        assert!(
            matches!(without.status, AuditStatus::Fail(_)),
            "{:?}",
            without.status
        );

        let with = run_with_not_destructive(CLEAN_AND_DELETE_CLI, &["clean"]);

        assert_eq!(with.status, AuditStatus::Pass);
        assert_eq!(
            with.mitigation,
            Some(Mitigation::Config(
                "declared not destructive: clean via .anc.toml [p5].not_destructive".into()
            ))
        );
    }

    #[test]
    fn declaring_every_destructive_subcommand_not_destructive_skips_and_names_them() {
        let script = r#"case "$*" in
  "clean --help") printf 'Usage: tool clean\n' ;;
  *) printf 'Usage: tool <COMMAND>\n\nCommands:\n  clean   Remove cached logs\n  list    List items\n' ;;
esac"#;

        match run_with_not_destructive(script, &["clean"]).status {
            AuditStatus::Skip(msg) => assert_eq!(
                msg,
                "no destructive subcommands remain; MUST applies conditionally to CLIs with \
                 destructive operations. Subcommands declared not destructive: clean via \
                 .anc.toml [p5].not_destructive."
            ),
            other => panic!("expected Skip, got {other:?}"),
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
