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
//! - `GIT_HARDEN_FLAGS` — five `-c key=value` pairs (`credential.helper`,
//!   `core.askPass`, `protocol.allow=never`, `protocol.https.allow=always`,
//!   `http.followRedirects`).
//! - `GIT_HARDEN_ENV_REMOVE` — five env vars stripped before spawn (SSH /
//!   proxy / askpass / exec-path overrides).
//! - `GIT_HARDEN_ENV_SET` — three env vars set on the spawned process:
//!   `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_SYSTEM=/dev/null`
//!   together disable every layer of user-controlled git config (the
//!   actual defense against `insteadOf` URL-rewriting attacks);
//!   `GIT_TERMINAL_PROMPT=0` blocks credential prompts.
//!
//! Never `env_clear()` (strips PATH, breaks git's helper resolution). Never
//! `sh -c` (tokens go directly to `git` via `Command::args`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::OutputFormat;
use crate::error::AppError;

// `SkillHost`, `KNOWN_HOSTS`, and `resolve_host` are auto-generated at
// build time from `src/skill_install/skill.json`. To add or remove a host,
// edit the JSON file (or run `bash scripts/sync-skill-fixture.sh` to pull
// the upstream site contract) and `cargo build` regenerates this file.
// See `build.rs::emit_skill_hosts` for the codegen logic.
include!(concat!(env!("OUT_DIR"), "/generated_hosts.rs"));

/// `git clone` config flags applied via `-c key=value` pairs, in token order
/// suitable for `Command::args`. Five logical pairs (10 string tokens):
///
/// | Pair | Purpose |
/// |------|---------|
/// | `credential.helper=` | Suppress credential helpers — public clone, no creds |
/// | `core.askPass=` | Suppress askpass programs |
/// | `protocol.allow=never` | Default-deny every transport |
/// | `protocol.https.allow=always` | Permit HTTPS only — paired with the deny above |
/// | `http.followRedirects=false` | Pin destination — no transparent redirects |
///
/// Two corrections over the plan's original wording, both surfaced by the
/// pre-merge manual smoke (R6c — verified against the actual `git` binary):
///
/// 1. `protocol.allow=https-only` is **not** valid git syntax (`fatal:
///    unknown value`). The HTTPS-only intent is expressed as a default-deny
///    plus per-protocol allow, which is the documented git-config form.
/// 2. `url.<repo>.insteadOf=` (empty value) does the **opposite** of
///    blocking — it rewrites every empty-prefix URL (i.e. all URLs) to
///    start with `<repo>`, doubling the clone URL. The defense against
///    `insteadOf` attacks is to disable global/system config entirely via
///    [`GIT_HARDEN_ENV_SET`] (`GIT_CONFIG_GLOBAL=/dev/null`,
///    `GIT_CONFIG_SYSTEM=/dev/null`), not via `-c`. The flag was dropped.
pub const GIT_HARDEN_FLAGS: &[&str] = &[
    "-c",
    "credential.helper=",
    "-c",
    "core.askPass=",
    "-c",
    "protocol.allow=never",
    "-c",
    "protocol.https.allow=always",
    "-c",
    "http.followRedirects=false",
];

/// Environment variables removed via `Command::env_remove` before spawn.
/// Each one is a known git-side override that could redirect or hijack the
/// clone. `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` are **not** in this
/// list — removing them would let git fall back to default config paths
/// (`~/.gitconfig`, `/etc/gitconfig`); we instead point them at `/dev/null`
/// via [`GIT_HARDEN_ENV_SET`] to actively disable user-controlled config.
/// Never `env_clear()` — that strips PATH and breaks git's helper
/// resolution.
pub const GIT_HARDEN_ENV_REMOVE: &[&str] = &[
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_PROXY_COMMAND",
    "GIT_ASKPASS",
    "GIT_EXEC_PATH",
];

/// Environment variables *set* (not removed) on the spawned process. The
/// `GIT_CONFIG_*` pair points global / system config at `/dev/null` so
/// user-config `insteadOf` rewriting and other ambient overrides cannot
/// fire — this is the actual defense against URL-rewriting attacks.
/// `GIT_TERMINAL_PROMPT=0` blocks credential prompts; git's default-when-
/// unset is to prompt, which is the wrong default for a non-interactive
/// subcommand.
pub const GIT_HARDEN_ENV_SET: &[(&str, &str)] = &[
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_CONFIG_SYSTEM", "/dev/null"),
    ("GIT_TERMINAL_PROMPT", "0"),
];

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
/// 1. [`GIT_HARDEN_FLAGS`] applied as `-c key=value` pairs *before* the
///    `clone` subcommand (git's required position for top-level `-c`).
/// 2. [`GIT_HARDEN_ENV_REMOVE`] entries removed via `env_remove` —
///    user/attacker-controlled overrides we want to ignore.
/// 3. [`GIT_HARDEN_ENV_SET`] entries set via `env` — most importantly
///    `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_SYSTEM=/dev/null`,
///    which together disable every layer of user-controlled git config
///    (the actual defense against `insteadOf` URL-rewriting attacks).
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
    for (key, value) in GIT_HARDEN_ENV_SET {
        cmd.env(key, value);
    }
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
    use clap::ValueEnum;

    /// Helper used by hardening-surface tests: any host's URL works because
    /// every install command in the v1 fixture shares one upstream. Reads
    /// straight from `resolve_host` so the value tracks the build-time
    /// codegen — no parallel hardcoded constant to keep in sync.
    fn skill_repo_url() -> &'static str {
        resolve_host(SkillHost::ClaudeCode).0
    }

    /// Test 1 — `resolve_host` returns the expected `(url, dest_template)`
    /// for every variant. Drives off `KNOWN_HOSTS` so adding a host to
    /// `skill.json` automatically extends coverage. Since both
    /// `KNOWN_HOSTS` and `resolve_host` are generated from the same JSON,
    /// this test catches build.rs codegen regressions (e.g. wrong URL,
    /// off-by-one tokenisation) — without it, a buggy emitter could
    /// produce arbitrary garbage and the rest of the suite would still
    /// pass.
    #[test]
    fn resolve_host_returns_expected_pair_for_every_variant() {
        let fixture_text = include_str!("skill_install/skill.json");
        let fixture: serde_json::Value =
            serde_json::from_str(fixture_text).expect("fixture is valid JSON");
        let install = fixture
            .get("install")
            .and_then(|v| v.as_object())
            .expect("fixture has install map");

        for &host_name in KNOWN_HOSTS {
            let cmd = install
                .get(host_name)
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("fixture missing install.{host_name}"));
            let tokens: Vec<&str> = cmd.split_whitespace().collect();
            let expected_url = tokens[4];
            let expected_dest = tokens[5];

            let host = SkillHost::from_str(host_name, false)
                .unwrap_or_else(|_| panic!("KNOWN_HOSTS entry {host_name:?} unparseable"));
            let (url, dest) = resolve_host(host);

            assert_eq!(url, expected_url, "url mismatch for {host_name}");
            assert_eq!(dest, expected_dest, "dest mismatch for {host_name}");
        }
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

    /// Defense-in-depth — the GIT_CONFIG_GLOBAL / GIT_CONFIG_SYSTEM
    /// pointers must be set to a path that disables config loading
    /// entirely (`/dev/null`). This is the actual defense against
    /// user-config `insteadOf` URL-rewriting attacks; an earlier draft
    /// used a `url.<repo>.insteadOf=` `-c` flag that did the *opposite*
    /// of blocking (it rewrote every URL to start with our repo, doubling
    /// the clone URL). Pin the corrected shape so we don't regress.
    #[test]
    fn git_harden_env_set_disables_user_config() {
        let pairs: std::collections::HashMap<&str, &str> =
            GIT_HARDEN_ENV_SET.iter().copied().collect();
        for var in ["GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM"] {
            let v = pairs.get(var).unwrap_or_else(|| {
                panic!("GIT_HARDEN_ENV_SET missing {var}; got {GIT_HARDEN_ENV_SET:?}")
            });
            assert_eq!(
                *v, "/dev/null",
                "{var} must be set to /dev/null to disable user config; got {v:?}",
            );
        }
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

    // Test 12 — `host_map_matches_site_skill_json` — was the cargo-level
    // drift anchor between the hand-maintained Rust map and the vendored
    // fixture. It is provably redundant after the build.rs codegen
    // refactor: both `SkillHost` / `KNOWN_HOSTS` / `resolve_host` AND
    // `tests/fixtures/skill.json` are now single-sourced from
    // `src/skill_install/skill.json`. Cargo's `rerun-if-changed` directive
    // ensures the codegen regenerates whenever the fixture changes, so
    // they cannot drift relative to each other within a single build.
    // Drift between the fixture and the upstream site contract is still
    // caught by `scripts/sync-skill-fixture.sh --check` (CI workflow
    // `skill-fixture-drift.yml`, test 26).

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
        let url = skill_repo_url();
        let dest = Path::new("/tmp/anc-skill-introspect");
        let cmd = build_clone_command(url, dest);

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
            args.iter().any(|a| a == url),
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

        for &(key, value) in GIT_HARDEN_ENV_SET {
            let entry = envs.get(key);
            assert_eq!(
                entry,
                Some(&Some(value.to_string())),
                "GIT_HARDEN_ENV_SET entry {key}={value:?} not present in env-set list; got {entry:?}",
            );
        }
    }

    /// Sanity: `format_clone_command` produces the canonical user-visible
    /// form, matching the `install.<host>` strings in `skill.json` once the
    /// destination template is expanded. Hardening flags are intentionally
    /// absent — implementation detail.
    #[test]
    fn format_clone_command_matches_canonical_shape() {
        let s = format_clone_command(
            skill_repo_url(),
            Path::new("/home/u/.claude/skills/agent-native-cli"),
        );
        assert_eq!(
            s,
            "git clone --depth 1 https://github.com/brettdavies/agentnative-skill.git /home/u/.claude/skills/agent-native-cli",
        );
    }
}
