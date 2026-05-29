//! Audit: `p1-must-secret-non-leaky-path`.
//!
//! Sensitive inputs (tokens, passwords, keys) are readable via stdin or a
//! `--*-file` companion flag. Flag-value and env-var paths MAY exist for
//! convenience but MUST NOT be the only path — process tables, shell history,
//! and CI logs all retain flag values.
//!
//! Detection strategy: scan `--help` for flags whose names look like they
//! receive secret material (`--token`, `--password`, `--api-key`, `--secret`,
//! `--auth`, `--credential`). For each detected flag, the audit passes when
//! either (a) a `*-file` companion appears in the same flag list, or (b) the
//! help text mentions stdin near the flag's name. Otherwise: Fail with the
//! offending flag named.
//!
//! When no secret-bearing flag is detected, the audit returns vacuous Pass —
//! the requirement only applies to CLIs that accept secret material.

use crate::audit::Audit;
use crate::project::Project;
use crate::runner::HelpOutput;
use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

/// Flag-name fragments that strongly imply the flag receives secret material.
/// Match is substring-on-long-form: a flag like `--api-token` matches `token`.
const SECRET_NAME_FRAGMENTS: &[&str] = &[
    "token",
    "password",
    "passwd",
    "secret",
    "api-key",
    "apikey",
    "auth-key",
    "credential",
    "private-key",
];

/// Tokens that, if mentioned anywhere in the help text, signal stdin support.
/// Conservative — we'd rather miss a stdin-supporting CLI (false Fail, easy to
/// override) than false-Pass a CLI that lacks the path entirely.
const STDIN_SIGNALS: &[&str] = &[
    "stdin",
    "STDIN",
    "standard input",
    "read from -",
    "from `-`",
];

pub struct SecretNonLeakyPathAudit;

impl Audit for SecretNonLeakyPathAudit {
    fn id(&self) -> &str {
        "p1-secret-non-leaky-path"
    }

    fn label(&self) -> &'static str {
        "Secret-bearing flags expose stdin or *-file companion"
    }

    fn group(&self) -> AuditGroup {
        AuditGroup::P1
    }

    fn layer(&self) -> AuditLayer {
        AuditLayer::Behavioral
    }

    fn covers(&self) -> &'static [&'static str] {
        &["p1-must-secret-non-leaky-path"]
    }

    fn applicable(&self, project: &Project) -> bool {
        project.runner.is_some()
    }

    fn run(&self, project: &Project) -> anyhow::Result<AuditResult> {
        let status = match project.help_output() {
            None => AuditStatus::Skip("could not probe --help".into()),
            Some(help) => audit_secret_non_leaky_path(help),
        };

        Ok(AuditResult {
            id: self.id().to_string(),
            label: self.label().into(),
            group: self.group(),
            layer: self.layer(),
            status,
            confidence: Confidence::Medium,
        })
    }
}

/// Core unit for tests. Walks the parsed flag list, identifies secret-bearing
/// flags, and verifies each one has a non-leaky companion.
pub(crate) fn audit_secret_non_leaky_path(help: &HelpOutput) -> AuditStatus {
    let flag_long_names: Vec<String> = help.flags().iter().filter_map(|f| f.long.clone()).collect();

    let secret_flags: Vec<&str> = flag_long_names
        .iter()
        .filter(|long| is_secret_flag(long))
        .map(|s| s.as_str())
        .collect();

    if secret_flags.is_empty() {
        return AuditStatus::Pass;
    }

    let raw = help.raw();
    let mentions_stdin = STDIN_SIGNALS.iter().any(|sig| raw.contains(sig));

    let mut leaky: Vec<&str> = Vec::new();
    for flag in &secret_flags {
        // Already a *-file flag itself (e.g., --token-file) — it IS the
        // non-leaky path. Skip the companion audit.
        if flag.ends_with("-file") {
            continue;
        }
        let file_companion = format!("{flag}-file");
        let has_companion = flag_long_names.iter().any(|f| f == &file_companion);
        if !has_companion && !mentions_stdin {
            leaky.push(flag);
        }
    }

    if leaky.is_empty() {
        AuditStatus::Pass
    } else {
        AuditStatus::Fail(format!(
            "secret-bearing flag(s) without `*-file` companion or stdin path: {}. \
             Flag values leak via process tables, shell history, and CI logs; \
             provide stdin support or a `--<flag>-file` variant.",
            leaky.join(", ")
        ))
    }
}

fn is_secret_flag(long: &str) -> bool {
    let stripped = long.trim_start_matches("--");
    SECRET_NAME_FRAGMENTS
        .iter()
        .any(|frag| stripped.contains(frag))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELP_TOKEN_WITH_FILE: &str = r#"Usage: tool [OPTIONS]

Options:
      --token <TOKEN>        API token used for authentication.
      --token-file <PATH>    Read API token from PATH (recommended for CI).
  -h, --help                 Show help.
"#;

    const HELP_TOKEN_BARE: &str = r#"Usage: tool [OPTIONS]

Options:
      --token <TOKEN>     API token used for authentication.
  -h, --help              Show help.
"#;

    const HELP_TOKEN_WITH_STDIN: &str = r#"Usage: tool [OPTIONS]

Reads the auth token from stdin when --token is not provided.

Options:
      --token <TOKEN>     API token used for authentication.
  -h, --help              Show help.
"#;

    const HELP_NO_SECRETS: &str = r#"Usage: tool [OPTIONS]

Options:
      --output <FORMAT>   Output format.
  -q, --quiet             Suppress output.
  -h, --help              Show help.
"#;

    const HELP_FILE_FLAG_ONLY: &str = r#"Usage: tool [OPTIONS]

Options:
      --secret-file <PATH>   Path to the secret material.
  -h, --help                 Show help.
"#;

    const HELP_PASSWORD_ONLY_LEAKY: &str = r#"Usage: tool [OPTIONS]

Options:
      --password <PASS>      Database password.
      --user <NAME>          Database user.
  -h, --help                 Show help.
"#;

    #[test]
    fn happy_path_token_with_file_companion() {
        let help = HelpOutput::from_raw(HELP_TOKEN_WITH_FILE);
        assert_eq!(audit_secret_non_leaky_path(&help), AuditStatus::Pass);
    }

    #[test]
    fn happy_path_token_with_stdin_mention() {
        let help = HelpOutput::from_raw(HELP_TOKEN_WITH_STDIN);
        assert_eq!(audit_secret_non_leaky_path(&help), AuditStatus::Pass);
    }

    #[test]
    fn happy_path_no_secrets_vacuous_pass() {
        let help = HelpOutput::from_raw(HELP_NO_SECRETS);
        assert_eq!(audit_secret_non_leaky_path(&help), AuditStatus::Pass);
    }

    #[test]
    fn happy_path_file_flag_only() {
        // A CLI that only exposes `--secret-file` (no bare `--secret`) is
        // already non-leaky — the file flag IS the companion.
        let help = HelpOutput::from_raw(HELP_FILE_FLAG_ONLY);
        assert_eq!(audit_secret_non_leaky_path(&help), AuditStatus::Pass);
    }

    #[test]
    fn fail_password_only_leaky() {
        let help = HelpOutput::from_raw(HELP_PASSWORD_ONLY_LEAKY);
        match audit_secret_non_leaky_path(&help) {
            AuditStatus::Fail(msg) => {
                assert!(
                    msg.contains("--password"),
                    "msg should name the flag: {msg}"
                );
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn fail_token_bare_no_companion_no_stdin() {
        let help = HelpOutput::from_raw(HELP_TOKEN_BARE);
        match audit_secret_non_leaky_path(&help) {
            AuditStatus::Fail(msg) => {
                assert!(msg.contains("--token"));
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn detects_apikey_fragment() {
        let raw = r#"Options:
      --apikey <KEY>     API key.
  -h, --help             Show help.
"#;
        let help = HelpOutput::from_raw(raw);
        match audit_secret_non_leaky_path(&help) {
            AuditStatus::Fail(msg) => assert!(msg.contains("--apikey")),
            other => panic!("expected Fail, got {other:?}"),
        }
    }
}
