//! Audit: `p2-must-json-errors`.
//!
//! When `--output json` is active, errors MUST be emitted as JSON to stderr
//! (or stdout, for tools that diverge from the spec's stderr placement) with
//! enough structure for an agent to dispatch on. A plain-text error inside a
//! JSON run breaks the consumer's parser on the only shape it was told to
//! expect.
//!
//! ## What "enough structure" means
//!
//! The envelope MUST carry three semantic roles, regardless of which field
//! names carry them:
//!
//! 1. **Discriminant** — a field that tells the agent "this is an error
//!    envelope, not success". Either a field named `error`/`status`/`kind`
//!    /`code`/`reason` (etc.) carrying a non-empty value, or any field whose
//!    value is one of `"error"`/`"err"`/`"failed"`/`"failure"`/`"fail"`,
//!    or `ok: false` / `success: false`.
//! 2. **Type identifier** — a kebab-case / snake_case / constant-case
//!    identifier value (no spaces, no period, ≤64 chars) somewhere in the
//!    envelope. Agents `match` on this; it must not be free-form prose.
//! 3. **Human-readable message** — prose text the CLI can surface to the
//!    user. Has whitespace, often ends with punctuation.
//!
//! Either the traditional `{error, kind, message}` shape OR the canonical
//! `{status, reason, exit_code, message}` shape (codified in
//! `anc-cli-output-envelope-pattern-2026-04-29.md` and dogfooded by
//! `anc skill install`) satisfies all three roles. The check measures
//! roles, not vocabulary, so it stops penalizing CLIs that pick reasonable
//! names anc didn't anticipate AND catches CLIs that have the right names
//! with the wrong shapes.
//!
//! Applicability gate: vacuous Skip when the binary doesn't advertise
//! `--output json` in its top-level `--help`. The MUST attaches to JSON
//! mode, so CLIs that never opted into the JSON contract are out of scope.

use crate::audit::Audit;
use crate::audits::behavioral::error_probe::{
    advertises_json_output, parse_error_envelope, probe_bad_invocation_json,
};
use crate::project::Project;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};
use serde_json::Value;

/// Field names whose presence (paired with an error-coded value) counts
/// as a discriminant signal. Case-insensitive match.
const DISCRIMINANT_FIELD_NAMES: &[&str] = &[
    "error", "status", "state", "outcome", "result", "kind", "type", "code", "reason", "severity",
    "level",
];

/// Closed-set string values that mean "this is an error" regardless of which
/// field carries them. Case-insensitive match.
const ERROR_VALUE_STRINGS: &[&str] = &["error", "err", "failed", "failure", "fail"];

/// Closed-set string values that mean "this is a SUCCESS". A
/// discriminant-named field carrying one of these is the gaming signal —
/// the envelope is claiming "error" via field name but "ok" via value. We
/// reject these from filling the discriminant role so the classifier
/// catches the mismatch.
const SUCCESS_VALUE_STRINGS: &[&str] = &[
    "ok",
    "success",
    "succeeded",
    "passed",
    "pass",
    "complete",
    "completed",
    "done",
];

/// Maximum length for a value to be considered a type identifier. Longer
/// strings are almost certainly prose, even if they look kebab-cased.
const MAX_TYPE_ID_LEN: usize = 64;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct RoleSet {
    discriminant: bool,
    type_id: bool,
    message: bool,
}

impl RoleSet {
    fn complete(self) -> bool {
        self.discriminant && self.type_id && self.message
    }

    fn missing_names(self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if !self.discriminant {
            out.push("discriminant (no field signals 'this is an error')");
        }
        if !self.type_id {
            out.push("type identifier (no kebab/snake/constant-case identifier value found)");
        }
        if !self.message {
            out.push("human-readable message (no prose value found)");
        }
        out
    }
}

fn classify_envelope(obj: &serde_json::Map<String, Value>) -> RoleSet {
    let mut roles = RoleSet::default();
    classify_object(obj, 0, &mut roles);
    roles
}

fn classify_object(obj: &serde_json::Map<String, Value>, depth: u32, roles: &mut RoleSet) {
    for (name, value) in obj {
        if !roles.discriminant && is_discriminant(name, value) {
            roles.discriminant = true;
        }
        if !roles.type_id && is_type_id_value(value) {
            roles.type_id = true;
        }
        if !roles.message && is_message_value(value) {
            roles.message = true;
        }
        if roles.complete() {
            return;
        }
        if depth < 1
            && let Some(nested) = value.as_object()
        {
            classify_object(nested, depth + 1, roles);
            if roles.complete() {
                return;
            }
        }
    }
}

fn is_discriminant(field_name: &str, value: &Value) -> bool {
    if let Some(s) = value.as_str()
        && ERROR_VALUE_STRINGS
            .iter()
            .any(|err| err.eq_ignore_ascii_case(s.trim()))
    {
        return true;
    }
    if matches!(value, Value::Bool(false)) {
        let name_lc = field_name.to_ascii_lowercase();
        if name_lc == "ok" || name_lc == "success" {
            return true;
        }
    }
    let name_lc = field_name.to_ascii_lowercase();
    if DISCRIMINANT_FIELD_NAMES
        .iter()
        .any(|n| n.eq_ignore_ascii_case(&name_lc))
    {
        match value {
            Value::String(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    return false;
                }
                // Gaming-case guard: a discriminant-named field carrying a
                // SUCCESS-coded value (`"ok"`, `"success"`, etc.) does NOT
                // fill the discriminant role. That's the envelope lying
                // about whether it's an error.
                if SUCCESS_VALUE_STRINGS
                    .iter()
                    .any(|ok| ok.eq_ignore_ascii_case(trimmed))
                {
                    return false;
                }
                return true;
            }
            Value::Number(_) => return true,
            _ => {}
        }
    }
    false
}

fn is_type_id_value(value: &Value) -> bool {
    let Some(s) = value.as_str() else {
        return false;
    };
    let trimmed = s.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_TYPE_ID_LEN {
        return false;
    }
    if trimmed
        .chars()
        .any(|c| c.is_whitespace() || c == '.' || c == ',' || c == ':' || c == ';')
    {
        return false;
    }
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_message_value(value: &Value) -> bool {
    let Some(s) = value.as_str() else {
        return false;
    };
    let trimmed = s.trim();
    if trimmed.len() < 4 {
        return false;
    }
    trimmed.contains(' ')
        || trimmed.ends_with('.')
        || trimmed.ends_with('!')
        || trimmed.ends_with('?')
}

pub struct JsonErrorsAudit;

impl Audit for JsonErrorsAudit {
    fn id(&self) -> &str {
        "p2-json-errors"
    }

    fn label(&self) -> &'static str {
        "Errors emit JSON envelope with discriminant + type id + message roles under `--output json`"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P2
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p2-must-json-errors"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) if !advertises_json_output(help) => AuditStatus::Skip(
                "binary does not advertise `--output json` in --help; \
                 MUST applies only to CLIs that opt into the JSON contract."
                    .into(),
            ),
            Some(_) => {
                let result = probe_bad_invocation_json(project.runner_ref());
                classify_json_error(&result)
            }
        };
        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::High,
            mitigation: None,
        })
    }
}

pub(crate) fn classify_json_error(result: &crate::runner::RunResult) -> AuditStatus {
    let Some((value, channel)) = parse_error_envelope(result) else {
        return AuditStatus::Fail(
            "bad invocation under `--output json` produced no parseable JSON \
             on stderr or stdout. JSON mode must emit a JSON error envelope, \
             not plain text."
                .into(),
        );
    };

    let Some(obj) = value.as_object() else {
        return AuditStatus::Fail(format!(
            "{channel} parsed as JSON but the top-level value is not an object. \
             Error envelopes must be `{{...}}` with named fields."
        ));
    };

    let roles = classify_envelope(obj);
    if roles.complete() {
        AuditStatus::Pass
    } else {
        let missing = roles.missing_names();
        AuditStatus::Fail(format!(
            "JSON error envelope on {channel} is missing role(s): {}. \
             The spec requires three semantic roles (discriminant, type \
             identifier, human-readable message) — the field names don't \
             matter as long as the value shapes do.",
            missing.join("; ")
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
    fn is_type_id_value_validates_json_shapes() {
        use serde_json::json;
        // Non-string values: number, bool, null, array, object — none are identifier-shaped.
        assert!(!is_type_id_value(&Value::Number(42.into())));
        assert!(!is_type_id_value(&Value::Bool(true)));
        assert!(!is_type_id_value(&Value::Null));
        assert!(!is_type_id_value(&json!([])));
        assert!(!is_type_id_value(&json!({})));
        // Valid identifier-shaped strings (kebab-case, SCREAMING_SNAKE, snake_case).
        assert!(is_type_id_value(&Value::String("auth-required".into())));
        assert!(is_type_id_value(&Value::String("NOT_FOUND".into())));
        assert!(is_type_id_value(&Value::String("rate_limit".into())));
        // Invalid string shapes (whitespace, punctuation, leading digit).
        assert!(!is_type_id_value(&Value::String("not an id".into())));
        assert!(!is_type_id_value(&Value::String("ends.with.dot".into())));
        assert!(!is_type_id_value(&Value::String(
            "9starts_with_digit".into()
        )));
    }

    #[test]
    fn pass_with_all_three_keys_on_stderr() {
        let stderr = r#"{"error":"BadFlag","kind":"usage","message":"unknown flag --bad"}"#;
        assert_eq!(
            classify_json_error(&fake_result(stderr, "")),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_with_extra_keys() {
        let stderr = r#"{"error":"BadFlag","kind":"usage","message":"unknown flag.","exit_code":2,"docs_url":"https://example.com/docs"}"#;
        assert_eq!(
            classify_json_error(&fake_result(stderr, "")),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_when_envelope_on_stdout() {
        let stdout = r#"{"error":"X","kind":"usage","message":"unknown flag found."}"#;
        assert_eq!(
            classify_json_error(&fake_result("", stdout)),
            AuditStatus::Pass
        );
    }

    #[test]
    fn fail_missing_type_id_role() {
        // Two prose-shaped fields, no kebab/snake identifier value.
        let stderr = r#"{"error":"Bad flag.","message":"unknown flag --bad."}"#;
        match classify_json_error(&fake_result(stderr, "")) {
            AuditStatus::Fail(msg) => {
                assert!(msg.contains("type identifier"));
                assert!(msg.contains("missing"));
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn pass_with_canonical_status_reason_exit_code_shape() {
        // anc's own envelope shape (per anc-cli-output-envelope-pattern doc).
        // Roles: status="error" (discriminant), reason="invalid-args" (type_id),
        // message="..." (message). Plus an extra exit_code for color.
        let stderr = r#"{"status":"error","reason":"invalid-args","exit_code":2,"message":"unexpected argument '--bogus'"}"#;
        assert_eq!(
            classify_json_error(&fake_result(stderr, "")),
            AuditStatus::Pass
        );
    }

    #[test]
    fn pass_with_ok_false_discriminant() {
        // Discriminant via `ok: false`, type_id via "code", message via "detail".
        let stderr = r#"{"ok":false,"code":"auth-required","detail":"Please authenticate."}"#;
        assert_eq!(
            classify_json_error(&fake_result(stderr, "")),
            AuditStatus::Pass
        );
    }

    #[test]
    fn fail_gaming_success_coded_values_in_error_envelope() {
        // Gaming case: three correctly-named fields but every value is a
        // SUCCESS-coded token. Old name-matching would PASS this. The
        // role-based check fails it because the SUCCESS_VALUE guard rejects
        // these values from filling the discriminant role, and none of the
        // values are identifier-shaped (they're all in the success vocab).
        let stderr = r#"{"error":"ok","kind":"OK","message":"ok"}"#;
        match classify_json_error(&fake_result(stderr, "")) {
            AuditStatus::Fail(_) => {}
            other => panic!("expected Fail, got {other:?}"),
        }
        // Deeper gaming (`{"error":"ok","kind":"<prose>","message":"<token>"}`)
        // can still slip past because each value plays a different role from
        // the one its field name implies. Catching that requires field-name-
        // to-role coupling — a tightening planned for a follow-up role-
        // coupling pass. The current check still catches every CLI that
        // emits a structurally-broken envelope and accepts the canonical
        // {status, reason, exit_code, message} shape that anc itself uses.
    }

    #[test]
    fn pass_with_nested_object_supplying_roles() {
        // Discriminant via top-level "status", type_id and message live in
        // a nested "error" object. One level of recursion catches this.
        let stderr =
            r#"{"status":"error","error":{"code":"NOT_FOUND","detail":"The post was not found."}}"#;
        assert_eq!(
            classify_json_error(&fake_result(stderr, "")),
            AuditStatus::Pass
        );
    }

    #[test]
    fn fail_only_message() {
        let stderr = r#"{"message":"Something went wrong."}"#;
        match classify_json_error(&fake_result(stderr, "")) {
            AuditStatus::Fail(_) => {} // missing discriminant + type_id
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn fail_only_type_id() {
        let stderr = r#"{"reason":"auth-required"}"#;
        match classify_json_error(&fake_result(stderr, "")) {
            AuditStatus::Fail(_) => {} // missing message; discriminant is the named field
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn fail_plain_text_error() {
        let stderr = "error: unknown flag --bad";
        match classify_json_error(&fake_result(stderr, "")) {
            AuditStatus::Fail(msg) => assert!(msg.contains("no parseable JSON")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn fail_when_top_level_is_array() {
        let stderr = r#"["error","kind","message"]"#;
        match classify_json_error(&fake_result(stderr, "")) {
            AuditStatus::Fail(msg) => assert!(msg.contains("not an object")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }
}
