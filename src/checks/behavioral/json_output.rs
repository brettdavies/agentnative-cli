use crate::check::Check;
use crate::project::Project;
use crate::runner::{BinaryRunner, RunStatus};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus};

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
                    validate_json_output(runner, has_output_flag, has_format_flag)
                } else {
                    CheckStatus::Skip("no --output/--format flag detected".into())
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
        })
    }
}

/// Try safe subcommands with the detected flag to validate actual JSON output.
///
/// Strategy: try `--help --flag json` first (safe — --help always exits without
/// side effects). If that produces non-JSON (many CLIs ignore --output with --help),
/// fall back to `--version --flag json`. Never run the binary with just `--flag json`
/// alone, as bare invocation could execute destructive commands.
fn validate_json_output(
    runner: &BinaryRunner,
    has_output_flag: bool,
    has_format_flag: bool,
) -> CheckStatus {
    // Build the list of flag variants to try, in priority order.
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

    // Safe subcommands to pair with the flag (never bare invocation).
    let safe_subcommands: &[&str] = &["--help", "--version"];

    for flag in &flag_variants {
        for subcommand in safe_subcommands {
            let result = runner.run(&[subcommand, flag, "json"], &[]);

            match result.status {
                RunStatus::Ok => {
                    // Check stdout for valid JSON
                    let stdout = result.stdout.trim();
                    if !stdout.is_empty()
                        && serde_json::from_str::<serde_json::Value>(stdout).is_ok()
                    {
                        return CheckStatus::Pass;
                    }

                    // Check stderr for valid JSON (some CLIs output JSON to stderr)
                    let stderr = result.stderr.trim();
                    if !stderr.is_empty()
                        && serde_json::from_str::<serde_json::Value>(stderr).is_ok()
                    {
                        // Non-zero exit but valid JSON on stderr is a warning
                        if result.exit_code != Some(0) {
                            return CheckStatus::Warn(
                                "binary exits non-zero but produces valid JSON on stderr".into(),
                            );
                        }
                        return CheckStatus::Pass;
                    }
                }
                // Non-Ok status (timeout, crash, etc.) — skip this combination, try next
                _ => continue,
            }
        }
    }

    // Also try `=json` syntax: `--output=json` / `--format=json`
    for flag in &flag_variants {
        let flag_eq = format!("{flag}=json");
        for subcommand in safe_subcommands {
            let result = runner.run(&[subcommand, &flag_eq], &[]);

            match result.status {
                RunStatus::Ok => {
                    let stdout = result.stdout.trim();
                    if !stdout.is_empty()
                        && serde_json::from_str::<serde_json::Value>(stdout).is_ok()
                    {
                        return CheckStatus::Pass;
                    }

                    let stderr = result.stderr.trim();
                    if !stderr.is_empty()
                        && serde_json::from_str::<serde_json::Value>(stderr).is_ok()
                    {
                        if result.exit_code != Some(0) {
                            return CheckStatus::Warn(
                                "binary exits non-zero but produces valid JSON on stderr".into(),
                            );
                        }
                        return CheckStatus::Pass;
                    }
                }
                _ => continue,
            }
        }
    }

    // Flag was detected but no combination produced valid JSON
    CheckStatus::Fail("--output/--format flag detected but no valid JSON produced".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::behavioral::tests::test_project_with_sh_script;
    use crate::types::CheckStatus;

    #[test]
    fn json_output_pass_with_valid_json() {
        // Script that advertises --output and produces valid JSON
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
        assert_eq!(
            result.status,
            CheckStatus::Pass,
            "expected Pass, got {:?}",
            result.status
        );
    }

    #[test]
    fn json_output_pass_with_format_flag() {
        // Script that advertises --format and produces valid JSON
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
        assert_eq!(
            result.status,
            CheckStatus::Pass,
            "expected Pass, got {:?}",
            result.status
        );
    }

    #[test]
    fn json_output_fail_with_invalid_json() {
        // Script that advertises --output but never produces valid JSON
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
            CheckStatus::Fail(msg) => {
                assert!(
                    msg.contains("no valid JSON"),
                    "expected 'no valid JSON' in message, got: {msg}"
                );
            }
            other => panic!("expected Fail, got {other:?}"),
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
        // Script where --help ignores --output but --version respects it
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
        assert_eq!(
            result.status,
            CheckStatus::Pass,
            "expected Pass, got {:?}",
            result.status
        );
    }

    #[test]
    fn json_output_handles_crash() {
        let project = test_project_with_sh_script("kill -11 $$");
        let result = JsonOutputCheck
            .run(&project)
            .expect("check should not panic on crash");
        assert!(matches!(result.status, CheckStatus::Skip(_)));
    }
}
