use crate::check::Check;
use crate::project::Project;
use crate::runner::{BinaryRunner, RunStatus};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

pub struct JsonOutputCheck;

impl Check for JsonOutputCheck {
    fn id(&self) -> &str {
        "p2-json-output"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P2
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p2-must-output-flag"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let runner = project.runner_ref();
        let help_result = runner.run(&["--help"], &[]);

        let status = match help_result.status {
            RunStatus::Ok => {
                let output = format!("{}{}", help_result.stdout, help_result.stderr);
                let lower = output.to_lowercase();
                let has_output_flag = lower.contains("--output");
                let has_format_flag = lower.contains("--format");

                if has_output_flag || has_format_flag {
                    // Flag found in top-level help, validate directly
                    validate_json_output(runner, &[], has_output_flag, has_format_flag)
                } else {
                    // Flag not in top-level help. Probe subcommands, since most CLIs
                    // (gh, kubectl, cargo) put --output on subcommands, not top-level.
                    probe_subcommands(runner, &output)
                }
            }
            _ => CheckStatus::Skip("could not run --help to detect output flags".into()),
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: "Structured output support".into(),
            group: CheckGroup::P2,
            layer: CheckLayer::Behavioral,
            status,
            confidence: Confidence::High,
        })
    }
}

/// Parse subcommand names from --help output and check each for --output/--format.
///
/// Most CLI frameworks (clap, cobra, argparse) list subcommands under a "Commands:"
/// or "Subcommands:" section. We parse those names and probe each one.
fn probe_subcommands(runner: &BinaryRunner, help_output: &str) -> CheckStatus {
    let subcommands = parse_subcommand_names(help_output);
    if subcommands.is_empty() {
        return CheckStatus::Skip("no --output/--format flag detected".into());
    }

    for subcmd in &subcommands {
        let sub_help = runner.run(&[subcmd, "--help"], &[]);
        if sub_help.status != RunStatus::Ok {
            continue;
        }

        let sub_output = format!("{}{}", sub_help.stdout, sub_help.stderr);
        let sub_lower = sub_output.to_lowercase();
        let has_output = sub_lower.contains("--output");
        let has_format = sub_lower.contains("--format");

        if has_output || has_format {
            return validate_json_output(runner, &[subcmd], has_output, has_format);
        }
    }

    CheckStatus::Skip("no --output/--format flag detected in any subcommand".into())
}

/// Extract subcommand names from CLI --help output.
///
/// Looks for a "Commands:" or "Subcommands:" section and parses the first word
/// of each indented line. Stops at the next section header or blank line gap.
fn parse_subcommand_names(help_output: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_commands_section = false;

    for line in help_output.lines() {
        let trimmed = line.trim();

        // Detect the start of a commands section
        if trimmed.eq_ignore_ascii_case("commands:")
            || trimmed.eq_ignore_ascii_case("subcommands:")
            || trimmed.starts_with("Commands:")
            || trimmed.starts_with("Subcommands:")
        {
            in_commands_section = true;
            continue;
        }

        if in_commands_section {
            // End of section: non-indented non-empty line (next section header)
            if !trimmed.is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
                break;
            }
            // Skip empty lines within the section
            if trimmed.is_empty() {
                continue;
            }
            // Extract the first word as the subcommand name
            if let Some(name) = trimmed.split_whitespace().next() {
                // Skip "help" subcommand (meta, not a real command)
                if name != "help" {
                    names.push(name.to_string());
                }
            }
        }
    }

    names
}

/// Try safe subcommands with the detected flag to validate actual JSON output.
///
/// `prefix` contains any subcommand path (e.g., ["check"]) to prepend to the
/// probe commands. For top-level flags, prefix is empty.
///
/// Strategy: try `[prefix...] --help --flag json` first (safe, --help always exits
/// without side effects). If that produces non-JSON (many CLIs ignore --output with
/// --help), fall back to `[prefix...] --version --flag json`. Never run the binary
/// bare with just `--flag json`, as that could execute destructive commands.
fn validate_json_output(
    runner: &BinaryRunner,
    prefix: &[&str],
    has_output_flag: bool,
    has_format_flag: bool,
) -> CheckStatus {
    let flag_variants: Vec<&str> = {
        let mut v = Vec::new();
        if has_output_flag {
            v.push("--output");
        }
        if has_format_flag {
            v.push("--format");
        }
        v
    };

    // Safe suffixes: always probe with --help or --version, never bare invocation.
    // Bare subcommand probing (`&[]`) was removed because it is unsafe in the
    // general case — subcommands may have side effects (kubectl apply, docker rm,
    // terraform plan), and for agentnative itself it caused fork bombs.
    let safe_suffixes: Vec<&[&str]> = vec![&["--help"], &["--version"]];

    // Try space-separated: [prefix...] [suffix...] flag json
    for flag in &flag_variants {
        for suffix in &safe_suffixes {
            let mut args: Vec<&str> = prefix.to_vec();
            args.extend_from_slice(suffix);
            args.push(flag);
            args.push("json");

            if let Some(status) = try_json_probe(runner, &args) {
                return status;
            }
        }
    }

    // Try =json syntax: [prefix...] [suffix...] flag=json
    for flag in &flag_variants {
        let flag_eq = format!("{flag}=json");
        for suffix in &safe_suffixes {
            let mut args: Vec<&str> = prefix.to_vec();
            args.extend_from_slice(suffix);
            let args_with_eq: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            let mut final_args: Vec<&str> = args_with_eq.iter().map(|s| s.as_str()).collect();
            final_args.push(&flag_eq);

            if let Some(status) = try_json_probe(runner, &final_args) {
                return status;
            }
        }
    }

    CheckStatus::Warn("--output/--format flag detected but could not validate JSON via safe probes (--help/--version override output flags in most CLIs)".into())
}

/// Run a single JSON probe and return Some(status) if valid JSON found.
fn try_json_probe(runner: &BinaryRunner, args: &[&str]) -> Option<CheckStatus> {
    let result = runner.run(args, &[]);

    match result.status {
        RunStatus::Ok => {
            let stdout = result.stdout.trim();
            if !stdout.is_empty() && serde_json::from_str::<serde_json::Value>(stdout).is_ok() {
                return Some(CheckStatus::Pass);
            }

            let stderr = result.stderr.trim();
            if !stderr.is_empty() && serde_json::from_str::<serde_json::Value>(stderr).is_ok() {
                if result.exit_code != Some(0) {
                    return Some(CheckStatus::Warn(
                        "binary exits non-zero but produces valid JSON on stderr".into(),
                    ));
                }
                return Some(CheckStatus::Pass);
            }

            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::behavioral::tests::test_project_with_sh_script;
    use crate::types::CheckStatus;

    #[test]
    fn json_output_pass_with_valid_json() {
        let script = r#"
case "$*" in
  *--help*--output*json*|*--output*json*--help*)
    echo '{"help":true,"format":"json"}';;
  *--help*)
    echo "Usage: test [--output FORMAT]";;
  *--output\ json*|*--output=json*)
    echo '{"version":"1.0"}';;
  *)
    echo "hello";;
esac
"#;
        let project = test_project_with_sh_script(script);
        let result = JsonOutputCheck.run(&project).expect("check should run");
        assert_eq!(result.status, CheckStatus::Pass, "got {:?}", result.status);
    }

    #[test]
    fn json_output_pass_with_format_flag() {
        let script = r#"
case "$*" in
  *--help*--format*json*|*--format*json*--help*)
    echo '{"help":true}';;
  *--help*)
    echo "Usage: test [--format FORMAT]";;
  *)
    echo "hello";;
esac
"#;
        let project = test_project_with_sh_script(script);
        let result = JsonOutputCheck.run(&project).expect("check should run");
        assert_eq!(result.status, CheckStatus::Pass, "got {:?}", result.status);
    }

    #[test]
    fn json_output_fail_with_invalid_json() {
        let script = r#"
case "$*" in
  *--help*)
    echo "Usage: test [--output FORMAT]";;
  *--output*)
    echo "this is not json";;
  *)
    echo "hello";;
esac
"#;
        let project = test_project_with_sh_script(script);
        let result = JsonOutputCheck.run(&project).expect("check should run");
        match &result.status {
            CheckStatus::Warn(msg) => {
                assert!(msg.contains("could not validate JSON"), "got: {msg}")
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn json_output_skip_no_flag() {
        let project = test_project_with_sh_script("echo 'just some help text'");
        let result = JsonOutputCheck.run(&project).expect("check should run");
        match &result.status {
            CheckStatus::Skip(msg) => assert!(msg.contains("no --output")),
            other => panic!("expected Skip, got {other:?}"),
        }
    }

    #[test]
    fn json_output_fallback_to_version() {
        let script = r#"
case "$*" in
  *--version*--output*json*|*--output*json*--version*|*--version*--output=json*|*--output=json*--version*)
    echo '{"version":"2.0"}';;
  *--help*)
    echo "Usage: test [--output FORMAT]";;
  *--version*)
    echo "test 2.0";;
  *)
    echo "hello";;
esac
"#;
        let project = test_project_with_sh_script(script);
        let result = JsonOutputCheck.run(&project).expect("check should run");
        assert_eq!(result.status, CheckStatus::Pass, "got {:?}", result.status);
    }

    #[test]
    fn json_output_handles_crash() {
        let project = test_project_with_sh_script("kill -11 $$");
        let result = JsonOutputCheck
            .run(&project)
            .expect("check should not panic on crash");
        assert!(matches!(result.status, CheckStatus::Skip(_)));
    }

    #[test]
    fn json_output_probes_subcommands() {
        // Simulates a CLI with subcommands where --output is on the subcommand.
        // More specific patterns must come before *--help* catch-all.
        let script = r#"
case "$*" in
  *check*--output*json*|*check*--output=json*)
    echo '{"checks":"passed"}';;
  *check*--help*)
    echo "Usage: test check [--output FORMAT]";;
  *--help*)
    echo "Usage: test [COMMAND]

Commands:
  check   Run checks
  list    List items
  help    Print help";;
  *)
    echo "hello";;
esac
"#;
        let project = test_project_with_sh_script(script);
        let result = JsonOutputCheck.run(&project).expect("check should run");
        // The check should find --output in the "check" subcommand help
        // and validate JSON output
        assert!(
            matches!(result.status, CheckStatus::Pass | CheckStatus::Fail(_)),
            "expected Pass or Fail (not Skip), got {:?}",
            result.status
        );
    }

    #[test]
    fn parse_subcommand_names_clap_format() {
        let help = "Usage: mycli [COMMAND]\n\nCommands:\n  check   Run checks\n  list    List items\n  help    Print help\n\nOptions:\n  -h, --help  Print help\n";
        let names = parse_subcommand_names(help);
        assert_eq!(names, vec!["check", "list"]);
    }

    #[test]
    fn parse_subcommand_names_empty() {
        let help = "Usage: mycli [OPTIONS]\n\nOptions:\n  -h, --help  Print help\n";
        let names = parse_subcommand_names(help);
        assert!(names.is_empty());
    }

    #[test]
    fn parse_subcommand_names_subcommands_header() {
        let help = "Subcommands:\n  run     Execute\n  build   Compile\n";
        let names = parse_subcommand_names(help);
        assert_eq!(names, vec!["run", "build"]);
    }
}
