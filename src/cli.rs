use clap::{Parser, Subcommand, ValueEnum, ValueHint};
use clap_complete::Shell;

use crate::principles::registry::ExceptionCategory;
use crate::skill_install::SkillHost;

#[derive(Parser)]
#[command(name = "anc", version, about = "The agent-native CLI linter")]
#[command(arg_required_else_help = true)]
#[command(
    after_help = "When the first argument is not a subcommand, `check` is inserted automatically:
  anc .                  ≡  anc check .
  anc --command ripgrep  ≡  anc check --command ripgrep

Bare `anc` (no arguments) prints this help and exits 2 — a deliberate guard
that prevents recursive self-invocation when agentnative checks itself."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Suppress non-essential output
    #[arg(long, short = 'q', global = true, env = "AGENTNATIVE_QUIET")]
    pub quiet: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Check a CLI project or binary for agent-readiness
    Check {
        /// Path to project directory or binary
        #[arg(default_value = ".")]
        path: std::path::PathBuf,

        /// Resolve a command from PATH and run behavioral checks against it
        #[arg(
            long,
            value_name = "NAME",
            value_hint = ValueHint::CommandName,
            conflicts_with = "path",
            conflicts_with = "source",
        )]
        command: Option<String>,

        /// Run only behavioral checks (skip source analysis)
        #[arg(long)]
        binary: bool,

        /// Run only source checks (skip behavioral)
        #[arg(long)]
        source: bool,

        /// Filter checks by principle number (1-7)
        #[arg(long)]
        principle: Option<u8>,

        /// Output format
        #[arg(long, default_value = "text")]
        output: OutputFormat,

        /// Include test code in source analysis
        #[arg(long)]
        include_tests: bool,

        /// Exemption category for the target. Suppresses checks that do not
        /// apply to this class of tool — e.g., TUI apps legitimately
        /// intercept the TTY, so `--audit-profile human-tui` skips the
        /// interactive-prompt MUSTs. Suppressed checks emit `Skip` with
        /// structured evidence so readers see what was excluded.
        #[arg(long, value_name = "CATEGORY")]
        audit_profile: Option<AuditProfile>,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate for
        shell: Shell,
    },
    /// Generate build artifacts (coverage matrix, etc.)
    Generate {
        #[command(subcommand)]
        artifact: GenerateKind,
    },
    /// Install or manage the agentnative skill bundle
    Skill {
        #[command(subcommand)]
        cmd: SkillCmd,
    },
}

#[derive(Subcommand)]
pub enum SkillCmd {
    /// Install the skill bundle into a host's canonical skills directory.
    ///
    /// If the site adds a host before this `anc` release knows about it, run
    /// the manual fallback printed by `--dry-run` for any known host and
    /// substitute the destination path:
    ///
    ///     git clone --depth 1 https://github.com/brettdavies/agentnative-skill.git <dest>
    Install {
        /// Target host (claude_code, codex, cursor, opencode).
        host: SkillHost,

        /// Print the resolved git command without spawning. Captures cleanly
        /// via `eval $(anc skill install --dry-run <host>)`.
        #[arg(long)]
        dry_run: bool,

        /// Output format for the result envelope.
        #[arg(long, default_value = "text")]
        output: OutputFormat,
    },
}

#[derive(Subcommand)]
pub enum GenerateKind {
    /// Render the spec coverage matrix (registry → checks → artifact).
    CoverageMatrix {
        /// Path for the Markdown artifact. Defaults to `docs/coverage-matrix.md`.
        #[arg(long, value_name = "PATH", default_value = "docs/coverage-matrix.md")]
        out: std::path::PathBuf,

        /// Path for the JSON artifact. Defaults to `coverage/matrix.json`.
        #[arg(
            long = "json-out",
            value_name = "PATH",
            default_value = "coverage/matrix.json"
        )]
        json_out: std::path::PathBuf,

        /// Exit non-zero when committed artifacts differ from generated output. CI drift guard.
        #[arg(long)]
        check: bool,
    },
}

#[derive(Clone, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

/// Exemption category for `--audit-profile`. Mirrors
/// `ExceptionCategory` in the registry one-to-one; the `From` impl below
/// converts between them at the call site. Kept as a CLI-owned type so
/// clap controls the surface (`value_enum` validation, shell completions)
/// without leaking clap into the registry module.
#[derive(Clone, Copy, ValueEnum, PartialEq, Eq, Debug)]
#[value(rename_all = "kebab-case")]
pub enum AuditProfile {
    /// TUI-by-design tools (lazygit, k9s, btop). Suppresses
    /// interactive-prompt MUSTs and SIGPIPE — their contract is the TTY.
    HumanTui,
    /// File-traversal utilities (fd, find). Reserved for subcommand-structure
    /// relaxations as those checks land.
    FileTraversal,
    /// POSIX utilities (cat, sed, awk). P1 interactive-prompt MUSTs
    /// satisfied vacuously via stdin-primary input.
    PosixUtility,
    /// Diagnostic tools (nvidia-smi, vmstat). No write operations, so the
    /// P5 mutation-boundary MUSTs do not apply.
    DiagnosticOnly,
}

impl From<AuditProfile> for ExceptionCategory {
    fn from(p: AuditProfile) -> Self {
        match p {
            AuditProfile::HumanTui => ExceptionCategory::HumanTui,
            AuditProfile::FileTraversal => ExceptionCategory::FileTraversal,
            AuditProfile::PosixUtility => ExceptionCategory::PosixUtility,
            AuditProfile::DiagnosticOnly => ExceptionCategory::DiagnosticOnly,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principles::registry::ALL_EXCEPTION_CATEGORIES;
    use clap::ValueEnum;

    /// Every CLI `AuditProfile` variant must map to a distinct
    /// `ExceptionCategory`, and every `ExceptionCategory` must be
    /// reachable from at least one `AuditProfile` variant. Failing this
    /// test means adding a category on one side without the other —
    /// either the CLI accepts a profile that suppresses nothing, or the
    /// registry defines a category no CLI user can reach.
    #[test]
    fn audit_profile_and_exception_category_variants_are_isomorphic() {
        let cli_mapped: std::collections::HashSet<&'static str> = AuditProfile::value_variants()
            .iter()
            .map(|v| ExceptionCategory::from(*v).as_kebab_case())
            .collect();
        let registry_kebab: std::collections::HashSet<&'static str> = ALL_EXCEPTION_CATEGORIES
            .iter()
            .map(|c| c.as_kebab_case())
            .collect();

        assert_eq!(
            cli_mapped, registry_kebab,
            "AuditProfile (cli) and ExceptionCategory (registry) variants must be isomorphic. \
             CLI-reachable: {cli_mapped:?}, registry: {registry_kebab:?}",
        );
    }

    /// The kebab-case string clap renders for each `AuditProfile` variant
    /// must equal the kebab-case the registry emits — otherwise the flag
    /// value a user types on the CLI won't match the `audit_profile` field
    /// echoed in JSON output.
    #[test]
    fn audit_profile_clap_name_matches_registry_kebab_case() {
        for variant in AuditProfile::value_variants() {
            let clap_name = variant
                .to_possible_value()
                .expect("AuditProfile variants have clap names")
                .get_name()
                .to_string();
            let registry_name = ExceptionCategory::from(*variant).as_kebab_case();
            assert_eq!(
                clap_name, registry_name,
                "clap value name and registry kebab-case must match for every variant",
            );
        }
    }
}
