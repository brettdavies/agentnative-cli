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

use clap::ValueEnum;

/// Canonical upstream URL for the skill bundle. Matched by the `insteadOf=`
/// blocker in `GIT_HARDEN_FLAGS` to defeat URL-rewriting attacks. Test 12
/// (the cargo-level drift anchor) asserts this matches the URL parsed from
/// the vendored `tests/fixtures/skill.json` for every host.
pub const SKILL_REPO_URL: &str = "https://github.com/brettdavies/agentnative-skill.git";

/// Host names accepted by `anc skill install <host>`, in declaration order.
/// Surfaces externally for shell-completion enumeration and as the seed for a
/// future `anc skill list` verb (R-LIST). Stays in lockstep with
/// [`SkillHost`] variants — test 11 enforces parity.
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
}
