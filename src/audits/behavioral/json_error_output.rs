//! Audit: `p4-should-json-error-output`.
//!
//! Errors must respect `--output json`: when JSON output is selected, error
//! messages go to stderr (or stdout, since some CLIs diverge there) as JSON.
//! Weaker than `p2-must-json-errors` — this SHOULD only requires the output
//! parse as JSON; it does not require the spec's `error`/`kind`/`message`
//! triple, since the stricter MUST already covers that.
//!
//! Applicability gate: vacuous Skip when the binary doesn't advertise
//! `--output json` in `--help`. Pass when stderr (or stdout) parses as JSON
//! under `--output json`; Warn when JSON mode is opted into but the error
//! comes out as plain text.

use crate::audit::Audit;
use crate::audits::behavioral::error_probe::{
    advertises_json_output, parse_error_envelope, probe_bad_invocation_json,
};
use crate::project::Project;
use crate::runner::RunResult;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

pub struct JsonErrorOutputAudit;

impl Audit for JsonErrorOutputAudit {
    fn id(&self) -> &str {
        "p4-json-error-output"
    }

    fn label(&self) -> &'static str {
        "`--output json` produces JSON-formatted errors"
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
            Some(help) if !advertises_json_output(help) => AuditStatus::Skip(
                "binary does not advertise `--output json` in --help; \
                 SHOULD applies only to CLIs that opt into the JSON contract."
                    .into(),
            ),
            Some(_) => {
                let result = probe_bad_invocation_json(project.runner_ref());
                classify_json_error_output(&result)
            }
        };
        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
        })
    }
}

pub(crate) fn classify_json_error_output(result: &RunResult) -> AuditStatus {
    if parse_error_envelope(result).is_some() {
        AuditStatus::Pass
    } else {
        AuditStatus::Warn(
            "errors under `--output json` are not JSON-formatted. \
             Consumers parsing stdout-as-JSON cannot recover the failure \
             without a separate text-parsing path; switch the error writer \
             to honor the active output mode."
                .into(),
        )
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
            classify_json_error_output(&fake_result(stderr, "")),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_on_json_stdout() {
        let stdout = r#"{"error":"BadFlag"}"#;
        assert_eq!(
            classify_json_error_output(&fake_result("", stdout)),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_even_without_spec_keys() {
        // SHOULD is weaker than the MUST — any parseable JSON passes here.
        let stderr = r#"{"reason":"x"}"#;
        assert_eq!(
            classify_json_error_output(&fake_result(stderr, "")),
            AuditStatus::Pass
        );
    }

    #[test]
    fn warn_on_plain_text() {
        let stderr = "error: bad flag\nFor more info, try --help.";
        match classify_json_error_output(&fake_result(stderr, "")) {
            AuditStatus::Warn(msg) => assert!(msg.contains("not JSON-formatted")),
            other => panic!("expected Warn, got {other:?}"),
        }
    }
}
