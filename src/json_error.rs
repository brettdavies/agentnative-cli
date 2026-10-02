//! JSON envelope renderer for clap-level errors, help, and version output
//! when the user requests JSON mode at the top level.
//!
//! Clap intercepts `--help`, `--version`, and parse failures before our
//! orchestration sees them, so the only way to honor `--output json` for
//! those paths is to detect JSON mode in raw argv and produce a JSON
//! envelope ourselves.
//!
//! Three envelope shapes share a non-payload key set so
//! `p2-should-consistent-envelope` Passes:
//!
//! - error: `{"kind":"usage","error":"<short>","message":"<full>"}`
//! - help:  `{"kind":"help","data":"<text>"}`
//! - version: `{"kind":"version","data":{"name":"...","version":"..."}}`
//!
//! Only `kind` is shared across all three. Per the consistent-envelope audit,
//! `data` is a recognized payload key so its absence from the error envelope
//! is not drift. Error envelopes additionally carry `error` and `message`,
//! which the success envelope lacks — the audit only flags keys present in
//! success but missing from error, so the asymmetric shape is intentional and
//! Passes.

use std::ffi::OsString;

use serde::Serialize;
use serde_json::json;

/// True iff the raw argv requests JSON output via `--json`, `--output json`,
/// or `--output=json`. Conservative: any token-form combination of the two
/// flags counts.
pub fn json_mode_in_argv(argv: &[OsString]) -> bool {
    let tokens: Vec<String> = argv
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        if t == "--json" {
            return true;
        }
        if t == "--output" {
            if let Some(next) = tokens.get(i + 1)
                && next == "json"
            {
                return true;
            }
            i += 2;
            continue;
        }
        if let Some(v) = t.strip_prefix("--output=")
            && v == "json"
        {
            return true;
        }
        i += 1;
    }
    false
}

/// The README section listing every flag, linked from a `show-help` step.
pub const CLI_REFERENCE_DOCS: &str = "https://github.com/brettdavies/agentnative-cli#cli-reference";

/// The README section on how a directory audit picks its binary, linked from
/// the binary-selection errors.
pub const BIN_SELECTION_DOCS: &str =
    "https://github.com/brettdavies/agentnative-cli#one-binary-several-or-none";

/// What the caller does next, carried by every error envelope as
/// `next_step`. A step carries `command`, runnable verbatim by a non-TTY
/// caller, or `template`, whose angle-bracket placeholder only the caller
/// can fill; never both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NextStep {
    pub action: NextAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs: Option<&'static str>,
}

/// The closed set of `next_step.action` values. A newer release can add
/// one, so a consumer treats an unknown action as its default branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NextAction {
    /// Read the help `command` prints.
    ShowHelp,
    /// Rerun with `--bin`: `template` holds the placeholder form, and the
    /// envelope's `candidates` hold one runnable command per binary.
    ChooseBin,
    /// Run `command`, the same invocation corrected.
    Rerun,
}

impl NextStep {
    pub fn show_help(command: String) -> Self {
        Self {
            action: NextAction::ShowHelp,
            command: Some(command),
            template: None,
            docs: Some(CLI_REFERENCE_DOCS),
        }
    }

    pub fn choose_bin(template: String) -> Self {
        Self {
            action: NextAction::ChooseBin,
            command: None,
            template: Some(template),
            docs: Some(BIN_SELECTION_DOCS),
        }
    }

    pub fn rerun(command: String) -> Self {
        Self {
            action: NextAction::Rerun,
            command: Some(command),
            template: None,
            docs: Some(BIN_SELECTION_DOCS),
        }
    }
}

/// Render a JSON error envelope as a single-line string. Required keys are
/// `error`, `kind`, and `message` per `p2-must-json-errors`. `exit_code` is
/// added so consumers can branch on it without re-deriving from the host
/// process's exit status, and `next_step` so they can recover without
/// parsing `message`.
pub fn render_error(
    kind: &str,
    error: &str,
    message: &str,
    exit_code: i32,
    next_step: &NextStep,
) -> String {
    render_error_with(
        kind,
        error,
        message,
        exit_code,
        next_step,
        serde_json::Map::new(),
    )
}

/// [`render_error`] plus further top-level fields, for an error whose
/// recovery needs structured data, such as the `candidates` of
/// `binary-ambiguous`, or that echoes the offending value.
pub fn render_error_with(
    kind: &str,
    error: &str,
    message: &str,
    exit_code: i32,
    next_step: &NextStep,
    extra: serde_json::Map<String, serde_json::Value>,
) -> String {
    let mut envelope = json!({
        "kind": kind,
        "error": error,
        "message": message,
        "exit_code": exit_code,
        "next_step": next_step,
    });
    if let Some(fields) = envelope.as_object_mut() {
        fields.extend(extra);
    }
    envelope.to_string()
}

/// The offending argument and value a clap error names, as envelope fields:
/// `argument` (the flag or positional) and `value` (the rejected value or
/// the unrecognized subcommand), each only when clap reports it.
pub fn clap_offending_fields(error: &clap::Error) -> serde_json::Map<String, serde_json::Value> {
    use clap::error::{ContextKind, ContextValue};
    let text = |kind| match error.get(kind) {
        Some(ContextValue::String(s)) => Some(s.clone()),
        Some(ContextValue::Strings(v)) => v.first().cloned(),
        _ => None,
    };
    let mut fields = serde_json::Map::new();
    if let Some(argument) = text(ContextKind::InvalidArg) {
        fields.insert("argument".into(), argument.into());
    }
    if let Some(value) =
        text(ContextKind::InvalidValue).or_else(|| text(ContextKind::InvalidSubcommand))
    {
        fields.insert("value".into(), value.into());
    }
    fields
}

/// Render a JSON help envelope wrapping clap's rendered text.
pub fn render_help(help_text: &str) -> String {
    json!({
        "kind": "help",
        "data": help_text,
    })
    .to_string()
}

/// Render a JSON version envelope.
pub fn render_version(name: &str, version: &str) -> String {
    json!({
        "kind": "version",
        "data": {
            "name": name,
            "version": version,
        },
    })
    .to_string()
}

/// Map a `clap::error::ErrorKind` to the kebab-cased `kind` and short
/// `error` identifier used in the JSON envelope. Every clap error is a
/// usage error from anc's perspective, so `kind` is always `"usage"` for
/// real parse failures; the `error` slug names the specific clap variant.
pub fn classify_clap_error(kind: clap::error::ErrorKind) -> (&'static str, &'static str) {
    use clap::error::ErrorKind as K;
    let slug = match kind {
        K::InvalidValue => "invalid-value",
        K::UnknownArgument => "unknown-argument",
        K::InvalidSubcommand => "invalid-subcommand",
        K::NoEquals => "no-equals",
        K::ValueValidation => "value-validation",
        K::TooManyValues => "too-many-values",
        K::TooFewValues => "too-few-values",
        K::WrongNumberOfValues => "wrong-number-of-values",
        K::ArgumentConflict => "argument-conflict",
        K::MissingRequiredArgument => "missing-required-argument",
        K::MissingSubcommand => "missing-subcommand",
        K::InvalidUtf8 => "invalid-utf8",
        K::DisplayHelp => "display-help",
        K::DisplayVersion => "display-version",
        K::DisplayHelpOnMissingArgumentOrSubcommand => "missing-subcommand",
        K::Io => "io",
        K::Format => "format",
        _ => "usage-error",
    };
    ("usage", slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn osv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn detects_json_global_flag() {
        assert!(json_mode_in_argv(&osv(&["anc", "--json", "audit", "."])));
    }

    #[test]
    fn detects_output_json_pair() {
        assert!(json_mode_in_argv(&osv(&[
            "anc", "audit", ".", "--output", "json"
        ])));
    }

    #[test]
    fn detects_output_equals_json() {
        assert!(json_mode_in_argv(&osv(&[
            "anc",
            "audit",
            ".",
            "--output=json",
        ])));
    }

    #[test]
    fn detects_output_json_at_end() {
        assert!(json_mode_in_argv(&osv(&[
            "anc",
            "--output",
            "json",
            "--this-flag-does-not-exist-anc",
        ])));
    }

    #[test]
    fn rejects_output_text() {
        assert!(!json_mode_in_argv(&osv(&[
            "anc", "audit", ".", "--output", "text"
        ])));
    }

    #[test]
    fn rejects_no_output_or_json() {
        assert!(!json_mode_in_argv(&osv(&["anc", "audit", "."])));
    }

    #[test]
    fn rejects_json_substring_in_value() {
        assert!(!json_mode_in_argv(&osv(&[
            "anc",
            "audit",
            ".",
            "--command",
            "json",
        ])));
    }

    fn parse_obj(s: &str) -> serde_json::Map<String, serde_json::Value> {
        let v: serde_json::Value =
            serde_json::from_str(s).expect("envelope under test is valid JSON");
        v.as_object()
            .expect("envelope under test serializes as a JSON object")
            .clone()
    }

    #[test]
    fn error_envelope_has_required_keys() {
        let s = render_error(
            "usage",
            "unknown-argument",
            "unknown flag --bad",
            2,
            &NextStep::show_help("anc audit --help".into()),
        );
        let obj = parse_obj(&s);
        assert!(obj.contains_key("error"));
        assert!(obj.contains_key("kind"));
        assert!(obj.contains_key("message"));
        assert_eq!(obj["exit_code"], 2);
        assert_eq!(
            obj["next_step"],
            json!({"action": "show-help", "command": "anc audit --help", "docs": CLI_REFERENCE_DOCS})
        );
    }

    #[test]
    fn a_step_carries_a_command_or_a_template_never_both() {
        let steps = [
            NextStep::show_help("anc --help".into()),
            NextStep::choose_bin("anc audit . --bin <name>".into()),
            NextStep::rerun("anc audit ./x".into()),
        ];
        for step in steps {
            assert!(
                step.command.is_some() != step.template.is_some(),
                "{step:?}"
            );
        }
    }

    #[test]
    fn help_envelope_is_a_json_object() {
        let s = render_help("Usage: anc ...");
        let obj = parse_obj(&s);
        assert_eq!(obj["kind"], "help");
        assert!(obj["data"].is_string());
    }

    #[test]
    fn version_envelope_has_name_and_version() {
        let s = render_version("anc", "0.5.0");
        let obj = parse_obj(&s);
        assert_eq!(obj["kind"], "version");
        assert_eq!(obj["data"]["name"], "anc");
        assert_eq!(obj["data"]["version"], "0.5.0");
    }

    #[test]
    fn classify_display_help_is_usage() {
        let (kind, slug) = classify_clap_error(clap::error::ErrorKind::DisplayHelp);
        assert_eq!(kind, "usage");
        assert_eq!(slug, "display-help");
    }
}
