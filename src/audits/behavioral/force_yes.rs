//! Audit: `p5-must-force-yes`.
//!
//! Destructive operations (those that delete or overwrite data the tool
//! cannot regenerate, or modify many records at once) MUST require an
//! explicit confirmation flag, `--yes` or `--force` by preference — agents
//! shouldn't be able to delete resources by guessing the call shape, and the
//! explicit flag makes the intent auditable in process tables and shell
//! history.
//!
//! P5 defines destructive by effect, and a name cannot show an effect, so the
//! audit detects by name and applies the author's declaration as a claim.
//!
//! Rubric: identify destructive subcommands via [`destructive_subcommands`],
//! less any the `.anc.toml` chain declares in `[p5] not_destructive`, probe
//! each one's `--help`, and audit for one of [`CONFIRM_FLAGS`], one of
//! [`DESCRIBED_CONFIRM_FLAGS`] whose help line says it confirms without a
//! prompt, or a flag the chain declares in `[p5] confirm_flags`. Fail when any
//! destructive subcommand lists none. Vacuous Skip when the binary has no
//! destructive subcommands.

use std::borrow::Cow;

use crate::anc_toml::{CONFIRM_FLAGS_KEY, NOT_DESTRUCTIVE_KEY, Sourced};
use crate::audit::Audit;
use crate::audits::behavioral::destructive_ops::destructive_subcommands;
use crate::audits::behavioral::flag_meaning::{self, Meaning};
use crate::audits::behavioral::subcommand_help::{dash_rule_notes, probe_subcommands};
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{
    AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence, Mitigation, Verdict, Waiver,
};

/// Flags that confirm a destructive operation non-interactively.
const CONFIRM_FLAGS: &[&str] = &["--force", "--yes", "-y", "-f"];

/// Names P5 does not give, which count under its clause for a flag under
/// another name: the command's `--help` documents it as confirming without a
/// prompt. pacman's `--confirm` asks for confirmation, and a `--confirm <ID>`
/// can confirm an order rather than the operation.
const DESCRIBED_CONFIRM_FLAGS: &[&str] = &["--auto-approve", "--assume-yes", "--confirm"];

/// What `-y` and `-f` must mean to confirm. `-f` is also `--file` (tar,
/// sed), `--filter` (lazygit) and `--format` (shellcheck). The words are the
/// accepted long names' own, plus "skip confirmation prompts". A flag the
/// `.anc.toml` chain declares is the operator's word and skips this check.
const CONFIRM: Meaning = Meaning {
    what: "force or yes",
    long_names: &[],
    words: &["force", "yes", "confirm", "confirmation"],
};

/// What a description says of a flag that confirms without a prompt. Each
/// phrase pairs the prompt with what removes it, because the bare words also
/// describe the opposite: `--confirm  Prompt for confirmation before deleting`.
const CONFIRMS_WITHOUT_PROMPT: Meaning = Meaning {
    what: "confirming without a prompt",
    long_names: &[],
    words: &[
        "answer yes",
        "assume yes",
        "auto approve",
        "auto confirm",
        "automatic yes",
        "automatically approve",
        "automatically approves",
        "automatically confirm",
        "automatically confirms",
        "bypass confirmation",
        "bypass the confirmation",
        "bypass the prompt",
        "do not ask",
        "do not prompt",
        "don't ask",
        "don't prompt",
        "never ask",
        "never prompt",
        "no confirmation",
        "no prompt",
        "no prompts",
        "non interactive",
        "non interactively",
        "noninteractive",
        "skip approval",
        "skip confirmation",
        "skip interactive",
        "skip prompt",
        "skip prompts",
        "skip the confirmation",
        "skip the prompt",
        "skips approval",
        "skips confirmation",
        "skips interactive",
        "skips the confirmation",
        "skips the prompt",
        "without a prompt",
        "without asking",
        "without confirmation",
        "without confirming",
        "without prompt",
        "without prompting",
        "without prompts",
    ],
};

pub struct ForceYesAudit;

impl Audit for ForceYesAudit {
    fn id(&self) -> &str {
        "p5-force-yes"
    }

    fn label(&self) -> &'static str {
        "Destructive subcommands require a confirmation flag"
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
                let declared = read_not_destructive(
                    &destructive_subcommands(top_help),
                    top_help.subcommands(),
                    not_destructive,
                );
                let verdict = if declared.destructive.is_empty() {
                    let found = if declared.waivers.is_empty() {
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
                    audit_force_yes(&declared.destructive, &subhelp, declared_flags)
                };
                with_not_destructive(verdict, declared)
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
            row_statuses: Vec::new(),
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
        if flag_meaning::find(help, CONFIRM_FLAGS, &CONFIRM).is_some()
            || flag_meaning::find_described(help, DESCRIBED_CONFIRM_FLAGS, &CONFIRMS_WITHOUT_PROMPT)
                .is_some()
        {
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
        .chain(DESCRIBED_CONFIRM_FLAGS)
        .copied()
        .chain(declared_names.iter().map(AsRef::as_ref))
        .collect();
    AuditStatus::Fail(format!(
        "destructive subcommand(s) whose --help lists no confirmation flag: {}. \
         Accepted flags: {}. Irreversible operations must require explicit \
         confirmation so they can't be invoked accidentally.{}{}",
        missing.join(", "),
        accepted_flags(declared_flags),
        dash_rule_notes(&missing, subhelp, &wanted, Some(&CONFIRM)),
        undescribed_notes(&missing, subhelp),
    ))
    .into()
}

/// What `[p5] not_destructive` did to the subcommands detected as destructive.
pub(crate) struct NotDestructive {
    /// The detected subcommands no entry waives.
    destructive: Vec<String>,
    waivers: Vec<Waiver>,
    /// Each entry not applied and why, cited with the file that declared it.
    unapplied: Vec<String>,
}

/// Apply `entries` to `detected`, comparing each with the lowercased
/// subcommand name. An entry that names a listed subcommand whose name does
/// not read as destructive has nothing to waive and is not an error.
pub(crate) fn read_not_destructive(
    detected: &[&String],
    listed: &[String],
    entries: &[Sourced<String>],
) -> NotDestructive {
    let mut destructive = Vec::new();
    let mut waivers = Vec::new();
    for name in detected {
        let lower = name.to_lowercase();
        match entries.iter().find(|entry| entry.value == lower) {
            Some(entry) => waivers.push(Waiver {
                subcommand: (*name).clone(),
                file: entry.file.clone(),
            }),
            None => destructive.push((*name).clone()),
        }
    }
    NotDestructive {
        destructive,
        waivers,
        unapplied: entries
            .iter()
            .filter_map(|entry| why_not_applied(entry, listed))
            .collect(),
    }
}

fn why_not_applied(entry: &Sourced<String>, listed: &[String]) -> Option<String> {
    let lower = entry.value.to_lowercase();
    let reason = if !listed.iter().any(|name| name.to_lowercase() == lower) {
        "the help lists no such subcommand".to_string()
    } else if entry.value != lower {
        format!("entries are lowercase, so write `{lower}`")
    } else {
        return None;
    };
    Some(format!(
        "`{}` in {} was not applied: {reason}",
        entry.value,
        entry.cite(NOT_DESTRUCTIVE_KEY)
    ))
}

/// Record what the declaration did: the waivers on the row, and the waived
/// subcommands and the entries not applied in the row's evidence, on a Pass
/// after any other setting it needed, or at the end of any other status.
fn with_not_destructive(verdict: Verdict, declared: NotDestructive) -> Verdict {
    let NotDestructive {
        waivers, unapplied, ..
    } = declared;
    let mut prose = Vec::new();
    let mut notes = Vec::new();
    if !waivers.is_empty() {
        let cited: Vec<String> = waivers
            .iter()
            .map(|waiver| {
                format!(
                    "{} via {} {NOT_DESTRUCTIVE_KEY}",
                    waiver.subcommand, waiver.file
                )
            })
            .collect();
        let waived = format!("declared not destructive: {}", cited.join(", "));
        notes.push(format!("Subcommands {waived}."));
        prose.push(waived);
    }
    for entry in unapplied {
        notes.push(format!("{entry}."));
        prose.push(entry);
    }
    if prose.is_empty() {
        return verdict;
    }
    let prose = prose.join("; ");
    Verdict {
        status: verdict.status.with_note(&notes.join(" ")),
        mitigation: Some(Mitigation::NotDestructive {
            waivers,
            evidence: match verdict.mitigation {
                Some(Mitigation::Config(earlier)) => format!("{earlier}; {prose}"),
                _ => prose,
            },
        }),
    }
}

/// The built-in names, the names that count by their description, then each
/// declared flag with the file it came from.
fn accepted_flags(declared_flags: &[Sourced<String>]) -> String {
    CONFIRM_FLAGS
        .iter()
        .map(|flag| (*flag).to_string())
        .chain(std::iter::once(format!(
            "{} (when its help says it confirms without a prompt)",
            DESCRIBED_CONFIRM_FLAGS.join(", ")
        )))
        .chain(
            declared_flags
                .iter()
                .map(|flag| format!("{} via {}", flag.value, flag.cite(CONFIRM_FLAGS_KEY))),
        )
        .collect::<Vec<_>>()
        .join(", ")
}

/// For each of `names`, the described flag its help declares without saying it
/// confirms without a prompt.
fn undescribed_notes(names: &[&str], subhelp: &[(String, HelpOutput)]) -> String {
    names
        .iter()
        .filter_map(|name| {
            let (_, help) = subhelp.iter().find(|(sub, _)| sub == name)?;
            let note = flag_meaning::undescribed_note(
                help,
                DESCRIBED_CONFIRM_FLAGS,
                &CONFIRMS_WITHOUT_PROMPT,
            )?;
            Some(format!(" In `{name}`, {note}"))
        })
        .collect()
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

    fn builtin(help: &str) -> AuditStatus {
        let subhelp = vec![("delete".to_string(), hp(help))];
        audit_force_yes(&["delete".to_string()], &subhelp, &[]).status
    }

    #[test]
    fn a_confirm_letter_passes_beside_an_accepted_long_name_or_a_description_that_names_it() {
        for help in [
            "Options:\n  -f, --force    skip confirmation prompts\n",
            "Options:\n  -y             Skip confirmation prompts.\n",
            "Options:\n  -f             Force removal.\n",
        ] {
            assert_eq!(builtin(help), AuditStatus::Pass, "{help}");
        }
    }

    #[test]
    fn a_confirm_letter_that_means_something_else_fails_and_says_why() {
        // tar 1.35's `-f`.
        match builtin("Options:\n  -f, --file=ARCHIVE         use archive file or device ARCHIVE\n")
        {
            AuditStatus::Fail(msg) => assert!(
                msg.ends_with(
                    "In `delete`, `-f` is declared, but its definition (`-f, --file=ARCHIVE  use \
                     archive file or device ARCHIVE`) does not mean force or yes."
                ),
                "{msg}"
            ),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn a_lone_confirm_letter_with_no_description_does_not_confirm() {
        match builtin("Options:\n  -y\n      --help    Show help.\n") {
            AuditStatus::Fail(msg) => assert!(
                msg.ends_with(
                    "In `delete`, `-y` is declared, but its definition (`-y`) does not mean \
                     force or yes."
                ),
                "{msg}"
            ),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn a_declared_letter_confirms_whatever_its_definition_says() {
        let verdict = confirms("Options:\n  -y, --yarn    Use yarn.\n", "y");
        assert_eq!(verdict.status, AuditStatus::Pass);
        assert_eq!(
            verdict.mitigation,
            Some(Mitigation::Config(
                "destroy accepts y via .anc.toml [p5].confirm_flags".into()
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
                msg.contains(
                    "--confirm (when its help says it confirms without a prompt), -auto-approve \
                     via .anc.toml [p5].confirm_flags."
                ),
                "{msg}"
            ),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn a_declared_not_destructive_subcommand_leaves_the_destructive_set() {
        let listed = ["clean".to_string(), "Purge".to_string()];
        let detected: Vec<&String> = listed.iter().collect();

        let read = read_not_destructive(&detected, &listed, &[declared("clean", ".anc.toml")]);

        assert_eq!(read.destructive, ["Purge"]);
        assert_eq!(
            read.waivers,
            [Waiver {
                subcommand: "clean".into(),
                file: ".anc.toml".into()
            }]
        );
        assert!(read.unapplied.is_empty(), "{:?}", read.unapplied);
    }

    #[test]
    fn a_lowercase_entry_waives_a_subcommand_the_help_spells_in_another_case() {
        let listed = ["Purge".to_string()];
        let detected: Vec<&String> = listed.iter().collect();

        let read = read_not_destructive(&detected, &listed, &[declared("purge", ".anc.toml")]);

        assert!(read.destructive.is_empty(), "{:?}", read.destructive);
        assert_eq!(read.waivers[0].subcommand, "Purge");
        assert!(read.unapplied.is_empty(), "{:?}", read.unapplied);
    }

    #[test]
    fn an_entry_for_a_listed_subcommand_that_reads_as_safe_has_nothing_to_waive() {
        let listed = ["clean".to_string(), "list".to_string()];
        let clean = listed[0].clone();

        let read = read_not_destructive(&[&clean], &listed, &[declared("list", ".anc.toml")]);

        assert_eq!(read.destructive, ["clean"]);
        assert!(read.waivers.is_empty(), "{:?}", read.waivers);
        assert!(read.unapplied.is_empty(), "{:?}", read.unapplied);
    }

    #[test]
    fn an_entry_in_another_case_naming_nothing_listed_says_the_help_lists_no_such_subcommand() {
        let listed = ["clean".to_string()];
        let clean = listed[0].clone();

        let read = read_not_destructive(&[&clean], &listed, &[declared("Purge", "~/.anc.toml")]);

        assert_eq!(
            read.unapplied,
            [
                "`Purge` in ~/.anc.toml [p5].not_destructive was not applied: the help lists no \
              such subcommand"
            ]
        );
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
            Some(Mitigation::NotDestructive {
                waivers: vec![Waiver {
                    subcommand: "clean".into(),
                    file: ".anc.toml".into()
                }],
                evidence: "declared not destructive: clean via .anc.toml [p5].not_destructive"
                    .into()
            })
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

    fn destroy_status(option_line: &str) -> AuditStatus {
        let subhelp = vec![(
            "destroy".to_string(),
            hp(&format!(
                "Usage: tool destroy\n\nOptions:\n      {option_line}\n  -h, --help    Show help.\n"
            )),
        )];
        audit_force_yes(&["destroy".to_string()], &subhelp, &[]).status
    }

    #[test]
    fn pass_on_described_confirmation_flags() {
        for flag in ["--auto-approve", "--assume-yes", "--confirm"] {
            assert_eq!(
                destroy_status(&format!("{flag}    Skip the prompt.")),
                AuditStatus::Pass,
                "{flag} confirms a destructive subcommand"
            );
        }
    }

    #[test]
    fn a_described_flag_passes_on_each_way_help_says_no_prompt() {
        for line in [
            "--confirm    Skip the confirmation prompt",
            "--confirm    Delete without prompting",
            "--confirm    Do not ask for confirmation",
            "--confirm    Don't ask before deleting",
            "--auto-approve    Skip interactive approval of plan before applying.",
            "--auto-approve    Automatically approve every step",
            "--assume-yes    Assume yes to all prompts",
            "--assume-yes    Automatic yes to prompts",
            "--assume-yes    Run non-interactively",
        ] {
            assert_eq!(destroy_status(line), AuditStatus::Pass, "{line}");
        }
    }

    #[test]
    fn a_described_flag_gives_no_credit_when_its_help_says_otherwise() {
        for line in [
            "--confirm <ID>    Confirm a pending order",
            // pacman's own line.
            "--confirm    Always ask for confirmation",
            "--confirm    Prompt for confirmation before deleting",
            "--auto-approve    Approve the plan",
            "--assume-yes",
        ] {
            match destroy_status(line) {
                AuditStatus::Fail(msg) => {
                    let name = line.split_whitespace().next().expect("a flag name");
                    assert!(
                        msg.contains(&format!(
                            "In `destroy`, `{name}` is declared, but its definition"
                        )) && msg.contains("does not mean confirming without a prompt."),
                        "{line}: {msg}"
                    );
                }
                other => panic!("{line}: expected Fail, got {other:?}"),
            }
        }
    }

    #[test]
    fn yes_and_force_pass_whatever_their_help_says() {
        for line in [
            "--yes    Proceed",
            "--force",
            "--force    Overwrite the target",
        ] {
            assert_eq!(destroy_status(line), AuditStatus::Pass, "{line}");
        }
    }

    #[test]
    fn a_declared_flag_skips_the_description_test() {
        let subhelp = vec![(
            "destroy".to_string(),
            hp("Options:\n      --confirm <ID>    Confirm a pending order\n"),
        )];
        let verdict = audit_force_yes(
            &["destroy".to_string()],
            &subhelp,
            &[declared("--confirm", ".anc.toml")],
        );
        assert_eq!(verdict.status, AuditStatus::Pass);
        assert_eq!(
            verdict.mitigation,
            Some(Mitigation::Config(
                "destroy accepts --confirm via .anc.toml [p5].confirm_flags".into()
            ))
        );
    }

    #[test]
    fn a_letter_beside_a_described_flag_answers_to_its_own_description() {
        assert_eq!(
            destroy_status("-y, --assume-yes    Assume yes"),
            AuditStatus::Pass
        );
        assert!(matches!(
            destroy_status("-c, --confirm <ID>    Confirm a pending order"),
            AuditStatus::Fail(_)
        ));
    }

    #[test]
    fn fail_names_the_accepted_flags() {
        let subhelp = vec![(
            "delete".to_string(),
            hp("Usage: tool delete <ID>\n\nOptions:\n  -h, --help    Show help.\n"),
        )];
        match audit_force_yes(&["delete".to_string()], &subhelp, &[]).status {
            AuditStatus::Fail(msg) => assert!(
                msg.contains(
                    "Accepted flags: --force, --yes, -y, -f, --auto-approve, --assume-yes, \
                     --confirm (when its help says it confirms without a prompt)."
                ),
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
