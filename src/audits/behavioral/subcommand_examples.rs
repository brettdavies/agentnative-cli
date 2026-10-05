//! Audit: `p3-must-subcommand-examples`.
//!
//! Every subcommand that takes arguments of its own ships at least one
//! concrete invocation example. Clap's `after_help` is the canonical
//! placement, but tools that hand-write help also satisfy the requirement
//! as long as the example line is present.
//!
//! A subcommand whose `--help` shows no operand, no command list, and no
//! flag beyond `-h` / `--help` and the flags the top-level help lists is
//! exempt: its usage line is its whole call shape. See
//! [`takes_own_arguments`] for how the help is read.
//!
//! Detection rubric: a `--help` body contains an example when any line
//! matches one of:
//!
//! - Starts with `$ ` (shell-prompt style — most common).
//! - Starts with the binary name (clap's `Examples:` block often renders
//!   `tool subcommand ...`).
//! - Lives inside a fenced code block (```` ``` ````).
//! - Contains the literal `Examples:` / `EXAMPLES` section header.
//!
//! Fail when any subcommand that takes arguments misses an example; the
//! evidence names the exempt subcommands too. Vacuous Skip when no
//! subcommand names were parsed from the top-level `--help`, or when every
//! subcommand that responded is exempt.

use crate::audit::Audit;
use crate::audits::behavioral::subcommand_arguments::takes_own_arguments;
use crate::audits::behavioral::subcommand_help::probe_subcommands;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::runner::help_probe::Flag;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct SubcommandExamplesAudit;

impl Audit for SubcommandExamplesAudit {
    fn id(&self) -> &str {
        "p3-subcommand-examples"
    }

    fn label(&self) -> &'static str {
        "Each subcommand that takes arguments ships an invocation example in its `--help`"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P3
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p3-must-subcommand-examples"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(top_help) if top_help.subcommands().is_empty() => AuditStatus::Skip(format!(
                "{}; the MUST applies conditionally to CLIs that use subcommands.",
                top_help.missing_subcommands_reason()
            )),
            Some(top_help) => {
                let runner = project.runner_ref();
                let subhelp = probe_subcommands(runner, top_help);
                let binary_name = project
                    .binary_paths
                    .first()
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str());
                audit_subcommand_examples(binary_name, top_help.flags(), &subhelp)
            }
        };
        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

pub(crate) fn audit_subcommand_examples(
    binary_name: Option<&str>,
    global_flags: &[Flag],
    subhelp: &[(String, HelpOutput)],
) -> AuditStatus {
    if subhelp.is_empty() {
        return AuditStatus::Skip(
            "no subcommands responded to `--help`; nothing to inspect.".into(),
        );
    }
    let (graded, exempt): (Vec<_>, Vec<_>) = subhelp
        .iter()
        .partition(|(name, help)| takes_own_arguments(name, help, global_flags));
    let exempt_note = if exempt.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = exempt.iter().map(|(name, _)| name.as_str()).collect();
        format!("exempt: {} (take no arguments)", names.join(", "))
    };
    if graded.is_empty() {
        return AuditStatus::Skip(format!(
            "{exempt_note}; the MUST applies conditionally to subcommands that accept arguments."
        ));
    }
    let missing: Vec<&str> = graded
        .iter()
        .filter(|(_, help)| !has_example_line(help.raw(), binary_name))
        .map(|(name, _)| name.as_str())
        .collect();
    if missing.is_empty() {
        return AuditStatus::Pass;
    }
    let mut evidence = format!(
        "subcommands missing example invocations in their `--help`: {}. \
         Examples teach agents the call shape faster than option tables; \
         use clap's `after_help` or a dedicated `Examples:` block.",
        missing.join(", ")
    );
    if !exempt_note.is_empty() {
        evidence.push(' ');
        evidence.push_str(&exempt_note);
        evidence.push('.');
    }
    AuditStatus::Fail(evidence)
}

/// True iff `raw` contains an example line. Heuristic — matches the four
/// shapes documented in the module header.
pub(crate) fn has_example_line(raw: &str, binary_name: Option<&str>) -> bool {
    let lower = raw.to_lowercase();
    if lower.contains("examples:") || lower.contains("\nexamples\n") {
        return true;
    }
    if raw.contains("```") {
        return true;
    }
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("$ ") {
            return true;
        }
        if let Some(name) = binary_name
            && !name.is_empty()
            && trimmed.starts_with(name)
            && trimmed.len() > name.len()
        {
            // Followed by whitespace, not just the bare name on its own line.
            let next = &trimmed[name.len()..];
            if next.starts_with(' ') {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hp(raw: &str) -> HelpOutput {
        HelpOutput::from_raw(raw)
    }

    #[test]
    fn pass_with_dollar_prompt() {
        let subhelp = vec![(
            "audit".to_string(),
            hp("Usage: tool audit [PATH]\n\n  $ tool audit .\n"),
        )];
        assert_eq!(
            audit_subcommand_examples(Some("tool"), &[], &subhelp),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_with_examples_header() {
        let subhelp = vec![(
            "audit".to_string(),
            hp("Usage: tool audit [PATH]\n\nExamples:\n  tool audit .\n"),
        )];
        assert_eq!(
            audit_subcommand_examples(Some("tool"), &[], &subhelp),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_with_binary_prefix() {
        let subhelp = vec![(
            "audit".to_string(),
            hp("Usage: tool audit [PATH]\n\nDescription...\n  tool audit . --output json\n"),
        )];
        assert_eq!(
            audit_subcommand_examples(Some("tool"), &[], &subhelp),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_with_fenced_block() {
        let subhelp = vec![(
            "audit".to_string(),
            hp("Usage: tool audit [PATH]\n\n```\ntool audit .\n```\n"),
        )];
        assert_eq!(
            audit_subcommand_examples(Some("tool"), &[], &subhelp),
            AuditStatus::Pass
        );
    }

    #[test]
    fn fail_when_subcommand_missing_example() {
        let subhelp = vec![
            (
                "audit".to_string(),
                hp("Usage: tool audit [PATH]\n\n$ tool audit .\n"),
            ),
            (
                "generate".to_string(),
                hp("Usage: tool generate <KIND>\n\nOptions: ...\n"),
            ),
        ];
        match audit_subcommand_examples(Some("tool"), &[], &subhelp) {
            AuditStatus::Fail(msg) => {
                assert!(msg.contains("generate"));
                assert!(!msg.contains(" audit,"), "audit should pass: {msg}");
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn skip_when_no_subhelp() {
        let subhelp: Vec<(String, HelpOutput)> = Vec::new();
        match audit_subcommand_examples(Some("tool"), &[], &subhelp) {
            AuditStatus::Skip(msg) => assert!(msg.contains("no subcommands")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn no_false_positive_on_bare_name_line() {
        // A line that is just "tool" (no trailing args) should not count
        // as an example — there's no invocation shape to learn from it.
        assert!(!has_example_line("Usage: tool\n\n  tool\n", Some("tool")));
    }

    const BIOME_TOP: &str = "Usage: biome COMMAND ...\n\n\
        Available options:\n    -h, --help     Prints help information\n    -V, --version  Prints version information\n\n\
        Available commands:\n    version        Prints the Biome CLI version\n    start          Starts the daemon server\n\
        \x20   stop           Stops the daemon server\n    clean          Removes the daemon server log files\n";
    const BIOME_STOP: &str = "Stops the Biome daemon server if it is running.\n\n\
        Usage: biome stop \n\nAvailable options:\n    -h, --help  Prints help information\n";
    const BIOME_CLEAN: &str = "Removes the Biome daemon server log files.\n\n\
        Usage: biome clean \n\nAvailable options:\n    -h, --help  Prints help information\n";
    const BIOME_START: &str = "Starts the Biome daemon server.\n\n\
        Usage: biome start [--log-prefix-name=STRING] [--log-path=PATH] [--log-level=\n\
        <none|tracing|debug|info|warn|error>] [--watcher-polling-interval=NUMBER]\n\n\
        Options that control internal CLI and daemon server logging.\n\
        \x20       --log-prefix-name=STRING  Sets the file name prefix used for rotated log files.\n\
        \x20       --log-path=PATH  Sets the directory where log files are stored.\n\n\
        Available options:\n    -h, --help           Prints help information\n";
    const BIOME_VERSION: &str = "Prints the Biome CLI version.\n\nUsage: biome version \n\n\
        These options are available to many commands and control general CLI behavior.\n\
        \x20       --colors=<off|force>  Controls ANSI styling.\n\
        \x20       --verbose             Shows additional diagnostic details.\n\n\
        Available options:\n    -h, --help                Prints help information\n";

    const TERRAFORM_TOP: &str = "Usage: terraform [global options] <subcommand> [args]\n\n\
        Main commands:\n  init          Prepare your working directory\n\n\
        All other commands:\n  logout        Remove locally-stored credentials\n  version       Show the current Terraform version\n\
        \x20 workspace     Workspace management\n\n\
        Global options (use these before the subcommand, if any):\n\
        \x20 -chdir=DIR    Switch to a different working directory.\n\
        \x20 -help         Show this help output.\n  -version      An alias for the \"version\" subcommand.\n";
    const TERRAFORM_VERSION: &str = "Usage: terraform [global options] version [options]\n\n\
        \x20 Displays the version of Terraform and all installed plugins\n\n\
        Options:\n\n  -json       Output the version information as a JSON object.\n";
    const TERRAFORM_LOGOUT: &str = "Usage: terraform [global options] logout [hostname]\n\n\
        \x20 Removes locally-stored credentials for specified hostname.\n\n\
        \x20 If no hostname is provided, the default hostname is app.terraform.io.\n";
    const TERRAFORM_WORKSPACE: &str = "Usage: terraform [global options] workspace\n\n\
        \x20 new, list, show, select and delete Terraform workspaces.\n\n\
        Subcommands:\n    delete    Delete a workspace\n    list      List Workspaces\n";
    // Terraform's usage shape for a subcommand that takes nothing; not a
    // real terraform subcommand.
    const TERRAFORM_SHAPED_ZERO_ARG: &str =
        "Usage: terraform [global options] status\n\n  Shows the status.\n";

    fn audit(binary: &str, top: &str, subhelp: &[(&str, &str)]) -> AuditStatus {
        let top = hp(top);
        let subhelp: Vec<(String, HelpOutput)> = subhelp
            .iter()
            .map(|(name, raw)| (name.to_string(), hp(raw)))
            .collect();
        audit_subcommand_examples(Some(binary), top.flags(), &subhelp)
    }

    fn fail_evidence(status: AuditStatus) -> String {
        match status {
            AuditStatus::Fail(evidence) => evidence,
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    fn missing_part(evidence: &str) -> &str {
        evidence.split(". ").next().unwrap_or(evidence)
    }

    #[test]
    fn skip_when_every_subcommand_takes_no_arguments() {
        let status = audit(
            "biome",
            BIOME_TOP,
            &[("stop", BIOME_STOP), ("clean", BIOME_CLEAN)],
        );
        assert_eq!(
            status,
            AuditStatus::Skip(
                "exempt: stop, clean (take no arguments); the MUST applies conditionally \
                 to subcommands that accept arguments."
                    .into()
            )
        );
    }

    #[test]
    fn subcommand_with_its_own_options_needs_an_example() {
        let evidence = fail_evidence(audit(
            "biome",
            BIOME_TOP,
            &[("start", BIOME_START), ("stop", BIOME_STOP)],
        ));
        assert!(missing_part(&evidence).ends_with(": start"), "{evidence}");
        assert!(
            evidence.ends_with(" exempt: stop (take no arguments)."),
            "{evidence}"
        );
    }

    #[test]
    fn options_absent_from_top_level_help_count_as_the_subcommands_own() {
        let evidence = fail_evidence(audit(
            "biome",
            BIOME_TOP,
            &[("version", BIOME_VERSION), ("stop", BIOME_STOP)],
        ));
        assert!(missing_part(&evidence).ends_with(": version"), "{evidence}");
        assert!(
            evidence.ends_with(" exempt: stop (take no arguments)."),
            "{evidence}"
        );
    }

    #[test]
    fn positional_or_command_specific_flag_needs_an_example() {
        let evidence = fail_evidence(audit(
            "terraform",
            TERRAFORM_TOP,
            &[
                ("logout", TERRAFORM_LOGOUT),
                ("version", TERRAFORM_VERSION),
                ("status", TERRAFORM_SHAPED_ZERO_ARG),
            ],
        ));
        assert!(
            missing_part(&evidence).ends_with(": logout, version"),
            "{evidence}"
        );
        assert!(
            evidence.ends_with(" exempt: status (take no arguments)."),
            "{evidence}"
        );
    }

    #[test]
    fn command_group_needs_an_example() {
        let evidence = fail_evidence(audit(
            "terraform",
            TERRAFORM_TOP,
            &[
                ("workspace", TERRAFORM_WORKSPACE),
                ("status", TERRAFORM_SHAPED_ZERO_ARG),
            ],
        ));
        assert!(
            missing_part(&evidence).ends_with(": workspace"),
            "{evidence}"
        );
        assert!(
            evidence.ends_with(" exempt: status (take no arguments)."),
            "{evidence}"
        );
    }

    #[test]
    fn global_flags_repeated_on_subcommand_help_are_not_its_own() {
        let top = "Usage:\n  scan [command]\n\nAvailable Commands:\n  version     display version\n\n\
            Flags:\n  -c, --config string   config file path\n  -h, --help            help for scan\n";
        let version = "display version\n\nUsage:\n  scan version [flags]\n\n\
            Flags:\n  -h, --help   help for version\n\n\
            Global Flags:\n  -c, --config string   config file path\n";
        assert!(matches!(
            audit("scan", top, &[("version", version)]),
            AuditStatus::Skip(msg) if msg.starts_with("exempt: version (take no arguments)")
        ));
    }

    #[test]
    fn pass_when_only_exempt_subcommands_lack_examples() {
        let format = "Usage: biome format [--write] [PATH]...\n\n\
            Examples:\n  biome format --write src\n";
        assert_eq!(
            audit(
                "biome",
                BIOME_TOP,
                &[("stop", BIOME_STOP), ("format", format)]
            ),
            AuditStatus::Pass
        );
    }
}
