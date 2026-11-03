//! Audit: `p4-should-json-error-output`.
//!
//! Errors must respect the JSON output mode: when JSON output is selected, error
//! messages go to stderr as JSON, so an agent reads them from one known stream.
//! This SHOULD checks the stream and that the output parses as JSON; the field
//! roles belong to `p2-must-json-errors`.
//!
//! Applicability gate: vacuous Skip when neither the top-level `--help` nor a
//! top-level subcommand's declares a JSON mode (`output_flags::json_mode`). Pass
//! when stderr parses as JSON under the declared mode; Warn when JSON mode is
//! opted into but the error comes out on stdout or as plain text.

use crate::audit::Audit;
use crate::audits::behavioral::error_probe::{NO_JSON_MODE, probe_bad_invocation_json};
use crate::audits::behavioral::output_flags;
use crate::project::Project;
use crate::runner::RunResult;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct JsonErrorOutputAudit;

impl Audit for JsonErrorOutputAudit {
    fn id(&self) -> &str {
        "p4-json-error-output"
    }

    fn label(&self) -> &'static str {
        "A JSON output mode produces JSON-formatted errors on stderr"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P4
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p4-should-json-error-output"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => match output_flags::json_mode(project.runner_ref(), help) {
                None => AuditStatus::Skip(format!(
                    "{NO_JSON_MODE}; SHOULD applies only to CLIs that opt into the JSON contract."
                )),
                Some(mode) => mode.noted(
                    match probe_bad_invocation_json(project.runner_ref(), &mode) {
                        Ok(result) => classify_json_error_output(&result, &mode.shown()),
                        Err(skip) => skip,
                    },
                ),
            },
        };
        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        })
    }
}

pub(crate) fn classify_json_error_output(result: &RunResult, mode: &str) -> AuditStatus {
    let parses = |text: &str| serde_json::from_str::<serde_json::Value>(text.trim()).is_ok();
    if parses(&result.stderr) {
        AuditStatus::Pass
    } else if parses(&result.stdout) {
        AuditStatus::Warn(format!(
            "errors under `{mode}` are JSON on stdout, not stderr. P4 sends JSON errors to stderr \
             (`p4-should-json-error-output`), so an agent reads them from one known stream; write \
             them to stderr."
        ))
    } else {
        AuditStatus::Warn(format!(
            "errors under `{mode}` are not JSON-formatted. Consumers parsing stdout-as-JSON \
             cannot recover the failure without a separate text-parsing path; switch the error \
             writer to honor the active output mode."
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::RunStatus;

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
    fn pass_on_json_stderr() {
        let stderr = r#"{"error":"BadFlag"}"#;
        assert_eq!(
            classify_json_error_output(&fake_result(stderr, ""), "--output json"),
            AuditStatus::Pass
        );
    }

    #[test]
    fn warn_on_json_stdout_with_plain_text_on_stderr() {
        let stdout = r#"{"error":"BadFlag"}"#;
        match classify_json_error_output(&fake_result("error: bad flag", stdout), "--output json") {
            AuditStatus::Warn(msg) => assert_eq!(
                msg,
                "errors under `--output json` are JSON on stdout, not stderr. P4 sends JSON errors \
                 to stderr (`p4-should-json-error-output`), so an agent reads them from one known \
                 stream; write them to stderr."
            ),
            other => panic!("expected Warn, got {other:?}"),
        }
    }

    #[test]
    fn warn_on_json_stdout_with_nothing_on_stderr() {
        let stdout = r#"{"error":"BadFlag"}"#;
        assert!(matches!(
            classify_json_error_output(&fake_result("", stdout), "--output json"),
            AuditStatus::Warn(_)
        ));
    }

    #[test]
    fn json_on_both_streams_passes_on_stderr() {
        let json = r#"{"error":"BadFlag"}"#;
        assert_eq!(
            classify_json_error_output(&fake_result(json, json), "--output json"),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_even_without_spec_keys() {
        // SHOULD is weaker than the MUST — any parseable JSON passes here.
        let stderr = r#"{"reason":"x"}"#;
        assert_eq!(
            classify_json_error_output(&fake_result(stderr, ""), "--output json"),
            AuditStatus::Pass
        );
    }

    #[test]
    fn warn_on_plain_text() {
        let stderr = "error: bad flag\nFor more info, try --help.";
        match classify_json_error_output(&fake_result(stderr, ""), "--output json") {
            AuditStatus::Warn(msg) => assert!(msg.contains("not JSON-formatted")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
