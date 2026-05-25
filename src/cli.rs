use clap::{Parser, Subcommand, ValueEnum, ValueHint};
use clap_complete::Shell;

use crate::principles::registry::ExceptionCategory;
use crate::skill_install::SkillHost;

#[derive(Parser)]
#[command(name = "anc", version, about = "The agent-native CLI linter")]
#[command(arg_required_else_help = true)]
#[command(
    long_about = "The agent-native CLI linter — audits a CLI tool against the agent-readiness spec.

Runs three layers of checks against a target: behavioral (spawn the binary and inspect output), source (ast-grep over Rust/Python files), and project (manifest, completions, bundle presence). The result is a scorecard you can read interactively (text mode) or pipe into another tool (`--output json`).

Default output format is text; color and progress affordances auto-detect the TTY and disappear when stdout is piped or redirected (NO_COLOR-compatible). Use `--verbose` / `-v` to escalate diagnostic detail when debugging unexpected results.

Input model: targets are passed as positional path arguments or via `--command <name>`. Stdin is not consumed; `-` is reserved and behaves like a literal filename rather than a stdin sentinel."
)]
#[command(after_help = "Examples:
  anc inspect .                          # human scorecard for the current project
  anc inspect . --output json            # JSON envelope for agents (--json works too)
  anc inspect --command ripgrep          # inspect a PATH-resolved binary by name
  anc emit coverage-matrix             # emit the spec coverage matrix
  anc emit schema                      # print the scorecard JSON Schema
  anc skill install claude_code          # install the bundle into Claude Code

When the first argument is not a subcommand, `inspect` is inserted automatically:
  anc .                  ≡  anc inspect .
  anc --command ripgrep  ≡  anc inspect --command ripgrep

Bare `anc` (no arguments) prints this help and exits 2 — a deliberate guard
that prevents recursive self-invocation when agentnative inspects itself.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Suppress non-essential output. Default: false (warnings and progress
    /// notes are written to stderr).
    #[arg(long, short = 'q', global = true, env = "AGENTNATIVE_QUIET")]
    pub quiet: bool,

    /// Escalate diagnostic detail. `-v` is shorthand for `--verbose`.
    /// Mutually exclusive with `--quiet`; the last flag on the command line
    /// wins when both appear.
    #[arg(
        long,
        short = 'v',
        global = true,
        env = "AGENTNATIVE_VERBOSE",
        conflicts_with = "quiet"
    )]
    pub verbose: bool,

    /// Print a curated examples block and exit. Equivalent to
    /// `anc examples` for tools that prefer a flag-shaped entry point.
    #[arg(long, global = true, exclusive = true)]
    pub examples: bool,

    /// Emit JSON output. Short alias for `--output json` on subcommands that
    /// support it. Per the agent-native convention (`p2-should-json-aliases`),
    /// the short form works alongside the canonical `--output` enum.
    #[arg(long, global = true)]
    pub json: bool,

    /// Color control for text output. `auto` (default) emits ANSI styling
    /// when stdout is a terminal and `NO_COLOR` is unset. `always` forces
    /// styling on; `never` strips it. Honors the `NO_COLOR` environment
    /// variable in `auto` mode (https://no-color.org/).
    #[arg(
        long,
        global = true,
        value_name = "WHEN",
        default_value = "auto",
        env = "AGENTNATIVE_COLOR"
    )]
    pub color: ColorChoice,

    /// Strip section headers, evidence lines, summary line, and badge hint
    /// — emit only `id<TAB>status` per check. Pipe-safe for grep, awk, and
    /// downstream tooling that wants the raw verdict stream without prose.
    /// Ignored in `--output json` mode.
    #[arg(long, global = true)]
    pub raw: bool,
}

/// `--color auto|always|never` — choice of when to emit ANSI styling.
/// Aligns with the cargo / rustc convention. `auto` consults TTY detection
/// and `NO_COLOR` at output time; the choice itself stays inert until
/// `format_text` queries `should_color()`.
#[derive(Clone, Copy, Debug, ValueEnum, Default, PartialEq, Eq)]
#[value(rename_all = "kebab-case")]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Inspect a CLI project or binary for agent-readiness
    ///
    /// Reads the target's project layout (Cargo.toml / pyproject.toml),
    /// language detection, and binary discovery. Stdin is not consumed.
    /// Pass the target as a positional argument or via `--command <name>`
    /// to resolve from PATH. `-` is reserved and behaves like any other
    /// path argument (no special stdin meaning).
    #[command(after_help = "Examples:
  anc inspect .                                  # default: project at cwd
  anc inspect . --output json                    # JSON envelope for agents
  anc inspect . --output json --principle 2      # filter to P2 (Structured Output)
  anc inspect --command ripgrep                  # PATH-resolved binary
  anc inspect ./target/release/anc --binary      # behavioral checks only

Defaults: path = `.`, output = text, no principle filter.")]
    Inspect {
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

        /// Filter checks by principle number (1-8)
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
    /// Render build artifacts (coverage matrix, scorecard schema)
    #[command(after_help = "Examples:
  anc emit coverage-matrix                            # write docs/coverage-matrix.md + coverage/matrix.json
  anc emit coverage-matrix --check                    # CI drift guard (non-zero on mismatch)
  anc emit coverage-matrix --out /tmp/cov.md          # custom output path
  anc emit schema                                     # print the scorecard JSON Schema to stdout
  anc emit schema | jq '.title'                       # pipe into jq for inspection")]
    Emit {
        #[command(subcommand)]
        artifact: EmitKind,
    },
    /// Install or manage the agentnative skill bundle
    ///
    /// Namespace for bundle operations. `anc skill install <host>` clones
    /// the agentnative-skill bundle into a host's canonical skills
    /// directory; `anc skill update <host>` refreshes an existing install.
    #[command(after_help = "Examples:
  anc skill install claude_code                # install bundle to Claude Code
  anc skill install claude_code --dry-run      # print the git command without spawning
  anc skill install --all                      # install across every known host
  anc skill update claude_code                 # refresh an existing install
  anc skill install codex --output json        # JSON envelope for agent consumption")]
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
        /// Target host (claude_code, codex, cursor, opencode). Required
        /// unless `--all` is set.
        host: Option<SkillHost>,

        /// Install into every known host in one invocation.
        #[arg(long, conflicts_with = "host")]
        all: bool,

        /// Print the resolved git command without spawning. Captures cleanly
        /// via `eval $(anc skill install --dry-run <host>)`.
        #[arg(long)]
        dry_run: bool,

        /// Output format for the result envelope.
        #[arg(long, default_value = "text")]
        output: OutputFormat,
    },
    /// Refresh an installed skill bundle to the latest upstream revision.
    Update {
        /// Target host. Required unless `--all` is set.
        host: Option<SkillHost>,

        /// Refresh every known host in one invocation.
        #[arg(long, conflicts_with = "host")]
        all: bool,

        /// Print the resolved commands without spawning.
        #[arg(long)]
        dry_run: bool,

        /// Output format for the result envelope.
        #[arg(long, default_value = "text")]
        output: OutputFormat,
    },
}

#[derive(Subcommand)]
pub enum EmitKind {
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

        /// Exit non-zero when committed artifacts differ from rendered output. CI drift guard.
        #[arg(long)]
        check: bool,
    },
    /// Print the scorecard JSON Schema (draft 2020-12) to stdout.
    ///
    /// Consumers (site renderer, leaderboards, agent integrations) validate
    /// scorecards against this contract instead of inferring shape from
    /// sample output. The schema is the same document committed at
    /// `schema/scorecard.schema.json` in this repo.
    Schema,
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
