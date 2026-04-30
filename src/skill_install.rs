//! `anc skill install <host>` — clone the `agentnative-skill` bundle into a
//! host's canonical skills directory using a hardcoded Rust host map and a
//! hardened `git clone` invocation.
//!
//! Pipeline (ASCII; the Mermaid version is plan-doc only — source comments
//! stay ASCII per repo convention):
//!
//! ```text
//!   clap parse (host, --dry-run, --output)
//!         |
//!         v
//!   resolve_host(SkillHost) -> (url, dest_template)
//!         |
//!         v
//!   expand_tilde(dest_template) via $HOME  -- HOME unset --> MissingHome
//!         |                                                  (reason=home-not-set)
//!         v
//!      dry_run? --yes--> emit_result(mode=dry-run, would_succeed) -> exit 0
//!         |
//!         no
//!         v
//!   check_destination()       -- conflict --> emit_result(error, reason) -> exit 1
//!    (canonicalize + R9)
//!         |
//!         v
//!   build_clone_command(url, dest) with hardening
//!    (GIT_HARDEN_FLAGS,
//!     env_remove GIT_HARDEN_ENV_REMOVE,
//!     set GIT_TERMINAL_PROMPT=0)
//!         |
//!         v
//!      spawn git -- not on PATH --> GitNotFound      (reason=git-not-found, exit 1)
//!         |     -- nonzero ------> GitCloneFailed    (reason=git-clone-failed, exit 1)
//!         v
//!      exit 0  ------------------> emit_result(mode=install, exit_code=0) -> exit 0
//! ```
//!
//! Hardening surface (R6c):
//! - `GIT_HARDEN_FLAGS` — five `-c key=value` pairs that defeat ambient git
//!   config (`credential.helper`, `core.askPass`, `protocol.allow`,
//!   `http.followRedirects`, `url.<repo>.insteadOf`).
//! - `GIT_HARDEN_ENV_REMOVE` — seven env vars stripped before spawn.
//! - `GIT_TERMINAL_PROMPT=0` is *set* (not removed) so git never prompts when
//!   credentials are missing — its default-when-unset is to prompt.
//!
//! Never `env_clear()` (strips PATH, breaks git's helper resolution). Never
//! `sh -c` (tokens go directly to `git` via `Command::args`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::ValueEnum;

use crate::cli::OutputFormat;
use crate::error::AppError;

/// Canonical upstream URL for the skill bundle. Matched by the `insteadOf=`
/// blocker in `GIT_HARDEN_FLAGS` to defeat URL-rewriting attacks. Test 12
/// (the cargo-level drift anchor) asserts this matches the URL parsed from
/// the vendored `tests/fixtures/skill.json` for every host.
pub const SKILL_REPO_URL: &str = "https://github.com/brettdavies/agentnative-skill.git";

/// Host names accepted by `anc skill install <host>`, in declaration order.
/// Surfaces externally for shell-completion enumeration and as the seed for a
/// future `anc skill list` verb (R-LIST). Stays in lockstep with
/// [`SkillHost`] variants — test 11 enforces parity. `#[allow(dead_code)]`
/// is intentional: no in-tree consumer in v1, but the constant is a
/// committed external API surface (see the R-LIST seed rationale in the
/// plan).
#[allow(dead_code)]
pub const KNOWN_HOSTS: &[&str] = &["claude_code", "codex", "cursor", "opencode"];

/// `git clone` config flags applied via `-c key=value` pairs, in token order
/// suitable for `Command::args`. Five logical pairs (10 string tokens):
///
/// | Pair | Purpose |
/// |------|---------|
/// | `credential.helper=` | Suppress credential helpers — public clone, no creds |
/// | `core.askPass=` | Suppress askpass programs |
/// | `protocol.allow=https-only` | Refuse `git://`, `ssh://`, `file://` etc. |
/// | `http.followRedirects=false` | Pin destination — no transparent redirects |
/// | `url.<repo>.insteadOf=` | Block ambient `insteadOf` rewriting for our URL |
pub const GIT_HARDEN_FLAGS: &[&str] = &[
    "-c",
    "credential.helper=",
    "-c",
    "core.askPass=",
    "-c",
    "protocol.allow=https-only",
    "-c",
    "http.followRedirects=false",
    "-c",
    "url.https://github.com/brettdavies/agentnative-skill.git.insteadOf=",
];

/// Environment variables removed via `Command::env_remove` before spawn.
/// Each one is a known git-side override that could redirect or hijack the
/// clone. Never `env_clear()` — that strips PATH and breaks git's own helper
/// resolution.
pub const GIT_HARDEN_ENV_REMOVE: &[&str] = &[
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_PROXY_COMMAND",
    "GIT_ASKPASS",
    "GIT_EXEC_PATH",
];

/// Env var *set* (not removed) on the spawned `git` process. Git's default
/// when this is unset is to prompt for input — wrong default for a
/// non-interactive subcommand.
pub const GIT_TERMINAL_PROMPT_KEY: &str = "GIT_TERMINAL_PROMPT";
pub const GIT_TERMINAL_PROMPT_VALUE: &str = "0";

/// Hosts the binary knows how to install into. Surface names match
/// `agentnative-site/src/data/skill.json` keys verbatim via
/// `rename_all = "snake_case"` — note `opencode` (one word), not `open_code`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum SkillHost {
    ClaudeCode,
    Codex,
    Cursor,
    Opencode,
}

/// Resolve a host enum to its `(url, dest_template)` pair. The URL is the
/// same for every host in v1; the destination template is host-specific and
/// `~`-prefixed. Pure function — no I/O, no side effects.
pub fn resolve_host(host: SkillHost) -> (&'static str, &'static str) {
    let dest_template = match host {
        SkillHost::ClaudeCode => "~/.claude/skills/agent-native-cli",
        SkillHost::Codex => "~/.codex/skills/agent-native-cli",
        SkillHost::Cursor => "~/.cursor/skills/agent-native-cli",
        SkillHost::Opencode => "~/.config/opencode/skills/agent-native-cli",
    };
    (SKILL_REPO_URL, dest_template)
}

/// Snapshot of what `check_destination` found at the resolved path. Drives
/// the JSON envelope's `destination_status` field. The success path returns
/// only `Absent`/`EmptyDir`; conflict cases (`NonEmptyDir`, `File`) are
/// inferred from the corresponding `AppError` variant in the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DestinationStatus {
    Absent,
    EmptyDir,
    NonEmptyDir,
    File,
}

impl DestinationStatus {
    /// Kebab-case identifier for the JSON envelope `destination_status` field.
    pub fn as_envelope_str(self) -> &'static str {
        match self {
            DestinationStatus::Absent => "absent",
            DestinationStatus::EmptyDir => "empty-dir",
            DestinationStatus::NonEmptyDir => "non-empty-dir",
            DestinationStatus::File => "file",
        }
    }
}

/// Expand a leading `~` or `~/` to `$HOME`. Pure passthrough on inputs that
/// do not start with `~` (R6a). `MissingHome` only fires when the input
/// actually begins with `~` and `$HOME` is unset or empty — non-`~` inputs
/// never read the environment.
pub fn expand_tilde(template: &str) -> Result<PathBuf, AppError> {
    let home = std::env::var("HOME").ok();
    expand_tilde_with(template, home.as_deref())
}

/// Pure-function core of [`expand_tilde`]. Tests pass `home` explicitly so
/// they never mutate the process environment (which would race with parallel
/// tests). The public wrapper performs the env lookup.
pub fn expand_tilde_with(template: &str, home: Option<&str>) -> Result<PathBuf, AppError> {
    let needs_home = template == "~" || template.starts_with("~/");
    if !needs_home {
        return Ok(PathBuf::from(template));
    }
    let home = home
        .filter(|s| !s.is_empty())
        .ok_or(AppError::MissingHome)?;
    if template == "~" {
        return Ok(PathBuf::from(home));
    }
    let rest = template
        .strip_prefix("~/")
        .expect("template starts with ~/ per the branch guard");
    let mut p = PathBuf::from(home);
    p.push(rest);
    Ok(p)
}

/// R9 destination conflict check. Canonicalizes the path so a symlinked
/// skills directory resolves to its real target before the check runs (F4).
/// Returns `Absent`/`EmptyDir` on success; `DestIsFile` for a regular file,
/// `DestNotEmpty` for a populated directory, `DestReadFailed` for any I/O
/// error along the way.
///
/// TOCTOU between this check and the subsequent `git clone` exec is
/// acknowledged residual single-user-machine risk — `git clone` itself
/// errors on a non-empty target, so the worst case is a less-actionable
/// error message, not a security failure.
pub fn check_destination(path: &Path) -> Result<DestinationStatus, AppError> {
    match path.try_exists() {
        Ok(false) => return Ok(DestinationStatus::Absent),
        Ok(true) => {}
        Err(e) => {
            return Err(AppError::DestReadFailed {
                path: path.to_path_buf(),
                source: e,
            });
        }
    }

    let canonical = fs::canonicalize(path).map_err(|e| AppError::DestReadFailed {
        path: path.to_path_buf(),
        source: e,
    })?;

    let metadata = fs::metadata(&canonical).map_err(|e| AppError::DestReadFailed {
        path: canonical.clone(),
        source: e,
    })?;

    if metadata.is_file() {
        return Err(AppError::DestIsFile { path: canonical });
    }

    if metadata.is_dir() {
        let mut entries = fs::read_dir(&canonical).map_err(|e| AppError::DestReadFailed {
            path: canonical.clone(),
            source: e,
        })?;
        if entries.next().is_some() {
            return Err(AppError::DestNotEmpty { path: canonical });
        }
        return Ok(DestinationStatus::EmptyDir);
    }

    // Block / char devices, sockets, fifos — not a normal place to clone into.
    // Treat as a file conflict; the typed reason maps to `destination-is-file`
    // which best describes "this is not the directory we expected".
    Err(AppError::DestIsFile { path: canonical })
}

/// Build the hardened `git clone` command. Pure constructor — no spawn, no
/// I/O. The returned `Command` carries the full hardening surface:
///
/// 1. `GIT_HARDEN_FLAGS` applied as `-c key=value` pairs *before* the
///    `clone` subcommand (git's required position for top-level `-c`).
/// 2. `GIT_HARDEN_ENV_REMOVE` entries removed via `env_remove`.
/// 3. `GIT_TERMINAL_PROMPT=0` set on the spawned process so git never
///    prompts for input.
///
/// `--depth 1` is included to match the canonical install command shipped
/// in `skill.json`. Verified during planning that `agentnative-skill`'s
/// `bin/check-update` curls the upstream `VERSION` file and does NOT
/// require local tag history, so shallow cloning is safe.
pub fn build_clone_command(url: &str, dest: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(GIT_HARDEN_FLAGS);
    cmd.args(["clone", "--depth", "1"]);
    cmd.arg(url);
    cmd.arg(dest);
    for var in GIT_HARDEN_ENV_REMOVE {
        cmd.env_remove(var);
    }
    cmd.env(GIT_TERMINAL_PROMPT_KEY, GIT_TERMINAL_PROMPT_VALUE);
    cmd
}

/// User-visible representation of the clone command for the JSON envelope's
/// `command` field and the `--dry-run --output text` single-line output.
/// Intentionally omits the hardening flags — those are an implementation
/// detail. The displayed form matches `skill.json`'s `install.<host>`
/// verbatim, so users can copy-paste-modify as a manual fallback.
pub fn format_clone_command(url: &str, dest: &Path) -> String {
    format!("git clone --depth 1 {url} {}", dest.display())
}

/// Result envelope shared by both `--output text` and `--output json`.
/// Schema is uniform across success and error paths (R-OUT, C1).
///
/// Field-presence rules:
/// - `would_succeed` — present in dry-run mode only.
/// - `exit_code` — present on the live install path only (and only when we
///   actually spawned `git`; e.g., `git-not-found` leaves it absent).
/// - `reason` — present on error only, with the typed values enumerated in
///   the plan's R-OUT.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InstallEnvelope {
    pub action: &'static str,
    pub host: &'static str,
    pub mode: &'static str,
    pub command: String,
    pub destination: String,
    pub destination_status: &'static str,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub would_succeed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

const ACTION: &str = "skill-install";
const MODE_DRY_RUN: &str = "dry-run";
const MODE_INSTALL: &str = "install";
const STATUS_SUCCESS: &str = "success";
const STATUS_ERROR: &str = "error";

const REASON_DEST_NOT_EMPTY: &str = "destination-not-empty";
const REASON_DEST_IS_FILE: &str = "destination-is-file";
const REASON_HOME_NOT_SET: &str = "home-not-set";
const REASON_GIT_NOT_FOUND: &str = "git-not-found";
const REASON_GIT_CLONE_FAILED: &str = "git-clone-failed";

fn host_envelope_str(host: SkillHost) -> &'static str {
    match host {
        SkillHost::ClaudeCode => "claude_code",
        SkillHost::Codex => "codex",
        SkillHost::Cursor => "cursor",
        SkillHost::Opencode => "opencode",
    }
}

/// Compute the envelope without performing I/O (dry-run) or, in install
/// mode, after spawning `git`. Internal I/O failures that don't fit the
/// typed reason taxonomy (e.g., `DestReadFailed` from a permission-denied
/// `read_dir`) propagate as `AppError` — the top-level handler renders
/// them on stderr and exits 2 (internal-error reserved per P4).
pub fn compute_install_envelope(
    host: SkillHost,
    dry_run: bool,
) -> Result<InstallEnvelope, AppError> {
    let (url, dest_template) = resolve_host(host);
    let host_str = host_envelope_str(host);
    let mode_str = if dry_run { MODE_DRY_RUN } else { MODE_INSTALL };

    // Step 1: tilde expand. MissingHome is an envelope error, not propagated.
    let dest = match expand_tilde(dest_template) {
        Ok(p) => p,
        Err(AppError::MissingHome) => {
            // Without $HOME we cannot show the resolved destination. Surface
            // the template (with its literal `~`) so the consumer sees what
            // would have been expanded; destination_status is `absent`
            // because we never reached the filesystem.
            let command = format!("git clone --depth 1 {url} {dest_template}");
            return Ok(InstallEnvelope {
                action: ACTION,
                host: host_str,
                mode: mode_str,
                command,
                destination: dest_template.to_string(),
                destination_status: DestinationStatus::Absent.as_envelope_str(),
                status: STATUS_ERROR,
                would_succeed: if dry_run { Some(false) } else { None },
                exit_code: None,
                reason: Some(REASON_HOME_NOT_SET),
            });
        }
        Err(e) => return Err(e),
    };

    let command = format_clone_command(url, &dest);
    let dest_str = dest.display().to_string();

    // Step 2: destination check. Conflict variants surface as envelope
    // errors; DestReadFailed propagates (internal I/O failure).
    let dest_status = match check_destination(&dest) {
        Ok(s) => s,
        Err(AppError::DestIsFile { .. }) => {
            return Ok(InstallEnvelope {
                action: ACTION,
                host: host_str,
                mode: mode_str,
                command,
                destination: dest_str,
                destination_status: DestinationStatus::File.as_envelope_str(),
                status: STATUS_ERROR,
                would_succeed: if dry_run { Some(false) } else { None },
                exit_code: None,
                reason: Some(REASON_DEST_IS_FILE),
            });
        }
        Err(AppError::DestNotEmpty { .. }) => {
            return Ok(InstallEnvelope {
                action: ACTION,
                host: host_str,
                mode: mode_str,
                command,
                destination: dest_str,
                destination_status: DestinationStatus::NonEmptyDir.as_envelope_str(),
                status: STATUS_ERROR,
                would_succeed: if dry_run { Some(false) } else { None },
                exit_code: None,
                reason: Some(REASON_DEST_NOT_EMPTY),
            });
        }
        Err(e) => return Err(e),
    };

    let dest_status_str = dest_status.as_envelope_str();

    if dry_run {
        return Ok(InstallEnvelope {
            action: ACTION,
            host: host_str,
            mode: MODE_DRY_RUN,
            command,
            destination: dest_str,
            destination_status: dest_status_str,
            status: STATUS_SUCCESS,
            would_succeed: Some(true),
            exit_code: None,
            reason: None,
        });
    }

    // Step 3: spawn `git`. The spawn helper produces typed `AppError`
    // variants (`GitNotFound`, `GitCloneFailed`) so the envelope-mapping
    // pattern stays uniform across error sources. Other I/O errors
    // propagate.
    let mut cmd = build_clone_command(url, &dest);
    match spawn_git_clone(&mut cmd) {
        Ok(()) => Ok(InstallEnvelope {
            action: ACTION,
            host: host_str,
            mode: MODE_INSTALL,
            command,
            destination: dest_str,
            destination_status: dest_status_str,
            status: STATUS_SUCCESS,
            would_succeed: None,
            exit_code: Some(0),
            reason: None,
        }),
        Err(AppError::GitCloneFailed { code }) => Ok(InstallEnvelope {
            action: ACTION,
            host: host_str,
            mode: MODE_INSTALL,
            command,
            destination: dest_str,
            destination_status: dest_status_str,
            status: STATUS_ERROR,
            would_succeed: None,
            exit_code: Some(code),
            reason: Some(REASON_GIT_CLONE_FAILED),
        }),
        Err(AppError::GitNotFound) => Ok(InstallEnvelope {
            action: ACTION,
            host: host_str,
            mode: MODE_INSTALL,
            command,
            destination: dest_str,
            destination_status: dest_status_str,
            status: STATUS_ERROR,
            would_succeed: None,
            exit_code: None,
            reason: Some(REASON_GIT_NOT_FOUND),
        }),
        Err(e) => Err(e),
    }
}

/// Spawn the prepared `git clone` command and reduce the result to typed
/// `AppError` variants matching the plan's reason taxonomy. Other I/O
/// failures (e.g., permission denied invoking the resolved binary) wrap
/// into `AppError::Io` and propagate.
fn spawn_git_clone(cmd: &mut Command) -> Result<(), AppError> {
    match cmd.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(AppError::GitCloneFailed {
            code: status.code().unwrap_or(-1),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(AppError::GitNotFound),
        Err(e) => Err(AppError::Io(e)),
    }
}

/// Render the envelope as a single-line text command (`git clone …`) on
/// success, or as a human error line on failure. Single-line dry-run output
/// is the contract that `eval $(anc skill install --dry-run <host>)`
/// depends on.
pub fn emit_result_text(env: &InstallEnvelope) -> String {
    if env.status == STATUS_SUCCESS {
        match env.mode {
            // Dry-run success: just the command, captures via `eval $(...)`.
            "dry-run" => env.command.clone(),
            // Live install success: short confirmation line on stdout.
            _ => format!("Installed agent-native-cli into {}", env.destination),
        }
    } else {
        let reason = env.reason.unwrap_or("unknown");
        format!("error: {reason}: {}", env.destination)
    }
}

/// Render the envelope as pretty-printed JSON. Pretty-print matches the
/// existing `anc check --output json` style and keeps grep / `jaq` queries
/// readable. `serde_json::to_string_pretty` is infallible for this struct
/// (no map keys, no non-string keys, no skipped serializer), so we
/// `expect()` rather than propagate.
pub fn emit_result_json(env: &InstallEnvelope) -> String {
    serde_json::to_string_pretty(env)
        .expect("InstallEnvelope serialization is infallible by construction")
}

/// Orchestrate the install pipeline. Always emits an envelope (text or
/// json) on stdout. Exit code is `0` for success, `1` for envelope errors
/// (typed reason set), and `AppError` for internal I/O failures handled by
/// `main` (exit 2 — internal-error reserved per P4).
pub fn run_install(host: SkillHost, dry_run: bool, output: OutputFormat) -> Result<i32, AppError> {
    let envelope = compute_install_envelope(host, dry_run)?;
    let rendered = match output {
        OutputFormat::Text => emit_result_text(&envelope),
        OutputFormat::Json => emit_result_json(&envelope),
    };
    println!("{rendered}");
    Ok(if envelope.status == STATUS_SUCCESS {
        0
    } else {
        1
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test 1 — `resolve_host` returns expected `(url, dest_template)` for
    /// every variant.
    #[test]
    fn resolve_host_returns_expected_pair_for_every_variant() {
        assert_eq!(
            resolve_host(SkillHost::ClaudeCode),
            (SKILL_REPO_URL, "~/.claude/skills/agent-native-cli")
        );
        assert_eq!(
            resolve_host(SkillHost::Codex),
            (SKILL_REPO_URL, "~/.codex/skills/agent-native-cli")
        );
        assert_eq!(
            resolve_host(SkillHost::Cursor),
            (SKILL_REPO_URL, "~/.cursor/skills/agent-native-cli")
        );
        assert_eq!(
            resolve_host(SkillHost::Opencode),
            (SKILL_REPO_URL, "~/.config/opencode/skills/agent-native-cli")
        );
    }

    /// Test 11 — `KNOWN_HOSTS` matches `SkillHost` variant count and names
    /// exactly. Catches the regression where someone adds a variant but
    /// forgets the const, or vice versa, before the next release ships.
    #[test]
    fn known_hosts_matches_skill_host_variant_count_and_names() {
        let variant_names: Vec<String> = SkillHost::value_variants()
            .iter()
            .map(|v| v.to_possible_value().unwrap().get_name().to_string())
            .collect();
        let known: Vec<String> = KNOWN_HOSTS.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(
            variant_names, known,
            "SkillHost variants and KNOWN_HOSTS must stay in lockstep",
        );
    }

    /// Defense-in-depth — the `insteadOf=` blocker in `GIT_HARDEN_FLAGS`
    /// must reference the same URL `resolve_host` uses, otherwise the
    /// blocker silently misses the target.
    #[test]
    fn git_harden_flags_insteadof_blocks_skill_repo_url() {
        let blocker = format!("url.{SKILL_REPO_URL}.insteadOf=");
        assert!(
            GIT_HARDEN_FLAGS.contains(&blocker.as_str()),
            "GIT_HARDEN_FLAGS missing insteadOf= blocker for {SKILL_REPO_URL}; got {GIT_HARDEN_FLAGS:?}",
        );
    }

    /// Sanity: no panics when constructing each variant via `ValueEnum`.
    /// Catches the regression where `rename_all = "snake_case"` is dropped
    /// and the surface names drift away from skill.json keys.
    #[test]
    fn skill_host_clap_value_names_match_known_hosts() {
        for &expected in KNOWN_HOSTS {
            let parsed = SkillHost::from_str(expected, false)
                .unwrap_or_else(|_| panic!("KNOWN_HOSTS entry {expected:?} not parseable"));
            let rendered = parsed.to_possible_value().unwrap().get_name().to_string();
            assert_eq!(rendered, expected);
        }
    }

    /// Test 2 — `expand_tilde("~/.claude/skills/agent-native-cli")` with
    /// `HOME=/home/test` resolves to the canonical absolute path. Uses the
    /// pure helper to avoid mutating process env (which would race with
    /// parallel tests).
    #[test]
    fn expand_tilde_replaces_leading_tilde_slash_with_home() {
        let got = expand_tilde_with("~/.claude/skills/agent-native-cli", Some("/home/test"))
            .expect("HOME present + ~/ prefix should expand cleanly");
        assert_eq!(
            got,
            PathBuf::from("/home/test/.claude/skills/agent-native-cli")
        );
    }

    /// Test 3 — `expand_tilde` with `HOME` unset returns `MissingHome`,
    /// but only when the input begins with `~`.
    #[test]
    fn expand_tilde_missing_home_only_when_input_starts_with_tilde() {
        let err = expand_tilde_with("~/anything", None)
            .expect_err("HOME unset + tilde input should be MissingHome");
        assert!(matches!(err, AppError::MissingHome));

        let err_empty =
            expand_tilde_with("~", Some("")).expect_err("HOME empty string is treated as unset");
        assert!(matches!(err_empty, AppError::MissingHome));
    }

    /// Test 4 — Passthrough contract: paths that don't start with `~` pass
    /// through unchanged regardless of `$HOME`. The hardcoded map only ever
    /// feeds `~`-prefixed templates, so this branch is unreachable in
    /// practice but keeps the contract simple and total (D1 passthrough).
    #[test]
    fn expand_tilde_no_tilde_passthrough() {
        let got_with_home = expand_tilde_with("/abs/path", Some("/home/test"))
            .expect("non-tilde input never errors");
        assert_eq!(got_with_home, PathBuf::from("/abs/path"));

        let got_without_home =
            expand_tilde_with("/abs/path", None).expect("non-tilde input ignores HOME");
        assert_eq!(got_without_home, PathBuf::from("/abs/path"));
    }

    /// Test 5 — `check_destination` on a nonexistent path returns
    /// `Absent`. A fresh tempdir's child is a deterministic nonexistent
    /// path.
    #[test]
    fn check_destination_absent_for_nonexistent_path() {
        let tmp = tempfile::tempdir().expect("tempdir creation");
        let target = tmp.path().join("does-not-exist");
        let status = check_destination(&target).expect("absent path should be Ok(Absent)");
        assert_eq!(status, DestinationStatus::Absent);
    }

    /// Test 6 — `check_destination` on an empty directory returns
    /// `EmptyDir`.
    #[test]
    fn check_destination_empty_dir() {
        let tmp = tempfile::tempdir().expect("tempdir creation");
        let status = check_destination(tmp.path()).expect("empty tempdir should be Ok(EmptyDir)");
        assert_eq!(status, DestinationStatus::EmptyDir);
    }

    /// Test 7 — `check_destination` on a non-empty directory returns
    /// `DestNotEmpty`.
    #[test]
    fn check_destination_non_empty_dir_errors() {
        let tmp = tempfile::tempdir().expect("tempdir creation");
        std::fs::write(tmp.path().join("placeholder"), b"x").expect("write placeholder");
        let err = check_destination(tmp.path()).expect_err("populated dir should be DestNotEmpty");
        assert!(matches!(err, AppError::DestNotEmpty { .. }));
    }

    /// Test 8 — `check_destination` on a regular file returns `DestIsFile`.
    #[test]
    fn check_destination_regular_file_errors() {
        let tmp = tempfile::tempdir().expect("tempdir creation");
        let target = tmp.path().join("a-file");
        std::fs::write(&target, b"contents").expect("write file");
        let err = check_destination(&target).expect_err("file should be DestIsFile");
        assert!(matches!(err, AppError::DestIsFile { .. }));
    }

    /// Test 9 — Symlink follow via `fs::canonicalize`: a symlink pointing at
    /// a non-empty directory returns `DestNotEmpty`, not the symlink's own
    /// status. Defends F4 — a symlinked skills dir resolves to the target
    /// before the conflict check runs.
    #[cfg(unix)]
    #[test]
    fn check_destination_follows_symlink_to_non_empty_dir() {
        let tmp = tempfile::tempdir().expect("tempdir creation");
        let real_dir = tmp.path().join("real");
        std::fs::create_dir(&real_dir).expect("mkdir real");
        std::fs::write(real_dir.join("placeholder"), b"x").expect("populate real");

        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&real_dir, &link).expect("symlink real -> link");

        let err = check_destination(&link).expect_err("symlinked non-empty dir is DestNotEmpty");
        assert!(matches!(err, AppError::DestNotEmpty { .. }));
    }

    /// Test 10 — `build_clone_command` introspection. Spawns nothing; reads
    /// the constructed `Command` via `get_args` / `get_envs`. Pins three
    /// invariants:
    ///
    /// 1. Every flag in `GIT_HARDEN_FLAGS` appears in the args list.
    /// 2. Every var in `GIT_HARDEN_ENV_REMOVE` is in the removal set
    ///    (`Some(None)` — set with no value means `env_remove`).
    /// 3. `GIT_TERMINAL_PROMPT=0` is in the env-set list.
    ///
    /// Also pins the conventional `clone --depth 1 <url> <dest>` shape.
    #[test]
    fn build_clone_command_applies_hardening_surface() {
        let dest = Path::new("/tmp/anc-skill-introspect");
        let cmd = build_clone_command(SKILL_REPO_URL, dest);

        let args: Vec<String> = cmd
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();

        for &flag in GIT_HARDEN_FLAGS {
            assert!(
                args.iter().any(|a| a == flag),
                "GIT_HARDEN_FLAGS entry {flag:?} missing from command args; got {args:?}",
            );
        }
        assert!(
            args.iter().any(|a| a == "clone"),
            "missing 'clone' subcommand: {args:?}"
        );
        assert!(
            args.iter().any(|a| a == "--depth"),
            "missing --depth flag: {args:?}"
        );
        assert!(
            args.iter().any(|a| a == "1"),
            "missing --depth value: {args:?}"
        );
        assert!(
            args.iter().any(|a| a == SKILL_REPO_URL),
            "missing url operand: {args:?}",
        );
        assert!(
            args.iter().any(|a| a == "/tmp/anc-skill-introspect"),
            "missing dest operand: {args:?}",
        );

        let envs: std::collections::HashMap<String, Option<String>> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|s| s.to_string_lossy().into_owned()),
                )
            })
            .collect();

        for &var in GIT_HARDEN_ENV_REMOVE {
            let entry = envs.get(var);
            assert!(
                matches!(entry, Some(None)),
                "GIT_HARDEN_ENV_REMOVE entry {var:?} should be removed; got {entry:?}",
            );
        }

        let prompt = envs.get(GIT_TERMINAL_PROMPT_KEY);
        assert_eq!(
            prompt,
            Some(&Some(GIT_TERMINAL_PROMPT_VALUE.to_string())),
            "GIT_TERMINAL_PROMPT must be set to 0; got {prompt:?}",
        );
    }

    /// Sanity: `format_clone_command` produces the canonical user-visible
    /// form, matching the `install.<host>` strings in `skill.json` once the
    /// destination template is expanded. Hardening flags are intentionally
    /// absent — implementation detail.
    #[test]
    fn format_clone_command_matches_canonical_shape() {
        let s = format_clone_command(
            SKILL_REPO_URL,
            Path::new("/home/u/.claude/skills/agent-native-cli"),
        );
        assert_eq!(
            s,
            "git clone --depth 1 https://github.com/brettdavies/agentnative-skill.git /home/u/.claude/skills/agent-native-cli",
        );
    }
}
