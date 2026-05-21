use crate::check::Check;
use crate::project::Project;
use crate::runner::{BinaryRunner, RunStatus};
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

/// Short-form `--version` aliases probed in priority order.
///
/// - `-V`: clap default, curl, wget, gzip, less.
/// - `-v`: npm, node, bun, yarn, make.
/// - `-version`: Go's `flag` package single-dash long form, Java tools.
///
/// Any one responding satisfies the SHOULD requirement.
const SHORT_VERSION_FLAGS: &[&str] = &["-V", "-v", "-version"];

pub struct VersionCheck;

impl Check for VersionCheck {
    fn id(&self) -> &str {
        "p3-version"
    }

    fn label(&self) -> &'static str {
        "Version flag works (`--version` plus short alias)"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P3
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p3-must-version", "p3-should-version-short"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let runner = project.runner_ref();

        let long = probe_version_flag(runner, "--version");
        let short_match = SHORT_VERSION_FLAGS
            .iter()
            .find(|flag| probe_version_flag(runner, flag).is_ok());

        let status = match (&long, short_match) {
            (Ok(()), Some(_)) => CheckStatus::Pass,
            (Ok(()), None) => CheckStatus::Warn(format!(
                "`--version` works but no short alias responded (tried {}). \
                 Adding one shortens version probes for agents.",
                SHORT_VERSION_FLAGS.join(", "),
            )),
            (Err(reason), _) => CheckStatus::Fail(reason.clone()),
        };

        Ok(CheckResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: CheckGroup::P3,
            layer: CheckLayer::Behavioral,
            status,
            confidence: Confidence::High,
        })
    }
}

/// Probe a single version flag against the target binary. Returns `Ok(())`
/// when the binary exits 0 with non-empty stdout (the same heuristic the
/// original `--version`-only check used). Returns `Err(reason)` otherwise,
/// with a human-readable failure cause suitable for evidence text.
fn probe_version_flag(runner: &BinaryRunner, flag: &str) -> Result<(), String> {
    let result = runner.run(&[flag], &[]);
    match result.status {
        RunStatus::Ok if result.exit_code == Some(0) && !result.stdout.trim().is_empty() => Ok(()),
        RunStatus::Ok if result.exit_code == Some(0) => Err(format!("`{flag}` produced no output")),
        RunStatus::Ok => Err(format!(
            "`{flag}` exited with code {}",
            result.exit_code.unwrap_or(-1)
        )),
        _ => Err(format!("`{flag}` failed: {:?}", result.status)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::behavioral::tests::test_project_with_runner;
    use crate::types::CheckStatus;

    #[test]
    fn version_pass_with_output() {
        // `echo` exits 0 with non-empty stdout for any args, so both
        // `--version` and any short alias produce a Pass-shaped response.
        let project = test_project_with_runner("/bin/echo");
        let result = VersionCheck.run(&project).expect("check should run");
        assert!(matches!(result.status, CheckStatus::Pass));
    }

    #[test]
    fn version_fail_non_zero_exit() {
        // `/bin/false` exits 1 regardless of args, so both probes fail and
        // the check returns Fail (the MUST is not satisfied).
        let project = test_project_with_runner("/bin/false");
        let result = VersionCheck.run(&project).expect("check should run");
        assert!(matches!(result.status, CheckStatus::Fail(_)));
    }

    #[test]
    fn version_handles_crash() {
        let project = crate::checks::behavioral::tests::test_project_with_sh_script("kill -11 $$");
        let result = VersionCheck
            .run(&project)
            .expect("check should not panic on crash");
        assert!(matches!(result.status, CheckStatus::Fail(_)));
    }

    #[test]
    fn version_warn_when_only_long_form_works() {
        // A shell script that responds to `--version` only emits a Warn:
        // the MUST is satisfied but no short alias responds.
        let script = "case \"$1\" in --version) echo 1.0.0 ;; *) exit 1 ;; esac";
        let project = crate::checks::behavioral::tests::test_project_with_sh_script(script);
        let result = VersionCheck.run(&project).expect("check should run");
        match result.status {
            CheckStatus::Warn(msg) => {
                assert!(
                    msg.contains("short alias"),
                    "warn message should mention the missing short alias, got: {msg}"
                );
            }
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn version_pass_when_short_alias_works() {
        // Both `--version` and `-V` exit 0 with output; everything else fails.
        // Verifies the SHORT_VERSION_FLAGS lookup returns first match.
        let script = "case \"$1\" in --version|-V) echo 1.0.0 ;; *) exit 1 ;; esac";
        let project = crate::checks::behavioral::tests::test_project_with_sh_script(script);
        let result = VersionCheck.run(&project).expect("check should run");
        assert!(matches!(result.status, CheckStatus::Pass));
    }
}
