//! Check: `p2-must-json-errors`.
//!
//! When `--output json` is active, errors MUST be emitted as JSON to stderr
//! (or stdout, for tools that diverge from the spec's stderr placement) with
//! at least `error`, `kind`, and `message` fields. A plain-text error inside
//! a JSON run breaks the consumer's parser on the only shape it was told to
//! expect.
//!
//! Applicability gate: vacuous Skip when the binary doesn't advertise
//! `--output json` in its top-level `--help`. The MUST attaches to JSON
//! mode, so CLIs that never opted into the JSON contract are out of scope.
//!
//! Strict reading of "at least error, kind, and message": all three are
//! required. Two-of-three is still Fail — the MUST words name three.

use crate::check::Check;
use crate::checks::behavioral::error_probe::{
    advertises_json_output, parse_error_envelope, probe_bad_invocation_json,
};
use crate::project::Project;
use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

/// Keys the JSON error envelope MUST contain.
const REQUIRED_KEYS: &[&str] = &["error", "kind", "message"];

pub struct JsonErrorsCheck;

impl Check for JsonErrorsCheck {
    fn id(&self) -> &str {
        "p2-json-errors"
    }

    fn label(&self) -> &'static str {
        "Errors emit JSON envelope with `error`/`kind`/`message` under `--output json`"
    }

    fn group(&self) -> CheckGroup {
        CheckGroup::P2
    }

    fn layer(&self) -> CheckLayer {
        CheckLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p2-must-json-errors"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<CheckResult> {
        let status = match project.help_output() {
            None => CheckStatus::Skip("could not probe --help".into()),
            Some(help) if !advertises_json_output(help) => CheckStatus::Skip(
                "binary does not advertise `--output json` in --help; \
                 MUST applies only to CLIs that opt into the JSON contract."
                    .into(),
            ),
            Some(_) => {
                let result = probe_bad_invocation_json(project.runner_ref());
                classify_json_error(&result)
            }
        };
        Ok(CheckResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

pub(crate) fn classify_json_error(result: &crate::runner::RunResult) -> CheckStatus {
    let Some((value, channel)) = parse_error_envelope(result) else {
        return CheckStatus::Fail(
            "bad invocation under `--output json` produced no parseable JSON \
             on stderr or stdout. JSON mode must emit a JSON error envelope, \
             not plain text."
                .into(),
        );
    };

    let Some(obj) = value.as_object() else {
        return CheckStatus::Fail(format!(
            "{channel} parsed as JSON but the top-level value is not an object. \
             Error envelopes must be `{{...}}` with named fields."
        ));
    };

    let missing: Vec<&&str> = REQUIRED_KEYS
        .iter()
        .filter(|k| !obj.contains_key(**k))
        .collect();
    if missing.is_empty() {
        CheckStatus::Pass
    } else {
        let missing_list: Vec<String> = missing.iter().map(|k| (**k).to_string()).collect();
        CheckStatus::Fail(format!(
            "JSON error envelope on {channel} is missing required keys: {}. \
             Spec requires at least `error`, `kind`, and `message`.",
            missing_list.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{RunResult, RunStatus};

    fn fake_result(stderr: &str, stdout: &str) -> RunResult {
        RunResult {
            exit_code: Some(2),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            status: RunStatus::Ok,
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }

    #[test]
    fn pass_with_all_three_keys_on_stderr() {
        let stderr = r#"{"error":"BadFlag","kind":"usage","message":"unknown flag --bad"}"#;
        assert_eq!(
            classify_json_error(&fake_result(stderr, "")),
            CheckStatus::Pass
        );
    }

    #[test]
    fn pass_with_extra_keys() {
        let stderr =
            r#"{"error":"BadFlag","kind":"usage","message":"...","exit_code":2,"docs_url":"..."}"#;
        assert_eq!(
            classify_json_error(&fake_result(stderr, "")),
            CheckStatus::Pass
        );
    }

    #[test]
    fn pass_when_envelope_on_stdout() {
        let stdout = r#"{"error":"X","kind":"usage","message":"m"}"#;
        assert_eq!(
            classify_json_error(&fake_result("", stdout)),
            CheckStatus::Pass
        );
    }

    #[test]
    fn fail_missing_kind() {
        let stderr = r#"{"error":"BadFlag","message":"unknown"}"#;
        match classify_json_error(&fake_result(stderr, "")) {
            CheckStatus::Fail(msg) => {
                assert!(msg.contains("kind"));
                assert!(msg.contains("missing"));
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn fail_plain_text_error() {
        let stderr = "error: unknown flag --bad";
        match classify_json_error(&fake_result(stderr, "")) {
            CheckStatus::Fail(msg) => assert!(msg.contains("no parseable JSON")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn fail_when_top_level_is_array() {
        let stderr = r#"["error","kind","message"]"#;
        match classify_json_error(&fake_result(stderr, "")) {
            CheckStatus::Fail(msg) => assert!(msg.contains("not an object")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }
}
