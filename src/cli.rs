use clap::{Parser, Subcommand, ValueEnum, ValueHint};
use clap_complete::Shell;

use crate::principles::registry::ExceptionCategory;

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
