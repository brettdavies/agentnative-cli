use clap::{Parser, Subcommand, ValueEnum, ValueHint};
use clap_complete::Shell;

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
