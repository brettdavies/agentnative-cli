use clap::{Parser, Subcommand, ValueEnum, ValueHint};
use clap_complete::Shell;

use crate::principles::registry::ExceptionCategory;
use crate::skill_install::SkillHost;

#[derive(Parser)]
#[command(name = "anc", version, about = "The agent-native CLI linter")]
#[command(arg_required_else_help = true)]
#[command(
    long_about = "The agent-native CLI linter — audits a CLI tool against the agent-readiness spec.

Runs three layers of audits against a target: behavioral (spawn the binary and inspect output), source (ast-grep over Rust/Python files), and project (manifest, completions, bundle presence). The result is a scorecard you can read interactively (text mode) or pipe into another tool (`--output json`).

Default output format is text; color and progress affordances auto-detect the TTY and disappear when stdout is piped or redirected (NO_COLOR-compatible). Use `--verbose` / `-v` to escalate diagnostic detail when debugging unexpected results.

Input model: targets are passed as positional path arguments or via `--command <name>`. Stdin is not consumed; `-` is reserved and behaves like a literal filename rather than a stdin sentinel."
)]
#[command(after_help = "Examples:
  anc audit .                          # human scorecard for the current project
  anc audit . --output json            # JSON envelope for agents (--json works too)
  anc audit --command ripgrep          # audit a PATH-resolved binary by name
  anc web localhost:8787               # audit a website, from this machine
  anc web anc.dev --output json        # the web scorecard for agents
  anc emit coverage-matrix             # emit the spec coverage matrix
  anc emit schema                      # print the scorecard JSON Schema
  anc skill install claude_code          # install the bundle into Claude Code

When the first argument is not a subcommand, one is inserted automatically —
`web` when the argument reads as a network target, `audit` otherwise:
  anc .                  ≡  anc audit .
  anc --command ripgrep  ≡  anc audit --command ripgrep
  anc anc.dev            ≡  anc web anc.dev
  anc localhost:8787     ≡  anc web localhost:8787
A path that exists always wins, so `anc ./anc.dev` audits the directory.

Exit codes:
  0  clean   1  warnings only   2  failures or usage error   3  could not check

Bare `anc` (no arguments) prints this help and exits 2 — a deliberate guard
that prevents recursive self-invocation when agentnative audits itself.")]
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

    /// Print a curated examples block and exit. Pairs with `--output json`
    /// (or `--json`) so structured-output consumers can fetch the examples
    /// without parsing the full `--help` body.
    #[arg(long, global = true)]
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
    /// — emit only `id<TAB>status` per audit. Pipe-safe for grep, awk, and
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
    /// Audit a CLI project or binary for agent-readiness
    ///
    /// Reads the target's project layout (Cargo.toml / pyproject.toml),
    /// language detection, and binary discovery. Stdin is not consumed.
    /// Pass the target as a positional argument or via `--command <name>`
    /// to resolve from PATH. `-` is reserved and behaves like any other
    /// path argument (no special stdin meaning).
    #[command(after_help = "Examples:
  anc audit .                                  # default: project at cwd
  anc audit . --output json                    # JSON envelope for agents
  anc audit . --output json --principle 2      # filter to P2 (Structured Output)
  anc audit --command ripgrep                  # PATH-resolved binary
  anc audit ./target/release/anc --binary      # behavioral audits only

Defaults: path = `.`, output = text, no principle filter.")]
    Audit {
        /// Path to project directory or binary
        #[arg(default_value = ".")]
        path: std::path::PathBuf,

        /// Resolve a command from PATH and run behavioral audits against it
        #[arg(
            long,
            value_name = "NAME",
            value_hint = ValueHint::CommandName,
            conflicts_with = "path",
            conflicts_with = "source",
        )]
        command: Option<String>,

        /// Run only behavioral audits (skip source analysis)
        #[arg(long)]
        binary: bool,

        /// Run only source audits (skip behavioral)
        #[arg(long)]
        source: bool,

        /// Filter audits by principle number (1-8)
        #[arg(long)]
        principle: Option<u8>,

        /// Output format
        #[arg(long, default_value = "text")]
        output: OutputFormat,

        /// Include test code in source analysis
        #[arg(long)]
        include_tests: bool,

        /// Exemption category for the target. Suppresses audits that do not
        /// apply to this class of tool — e.g., TUI apps legitimately
        /// intercept the TTY, so `--audit-profile human-tui` skips the
        /// interactive-prompt MUSTs. Suppressed audits emit `Skip` with
        /// structured evidence so readers see what was excluded.
        #[arg(long, value_name = "CATEGORY")]
        audit_profile: Option<AuditProfile>,
    },
    /// Audit a website for agent-readiness
    ///
    /// Runs the anc.dev web-audit registry against a target from this
    /// machine, so a localhost, private or internal site the hosted
    /// auditor cannot reach is auditable where it runs. Every probe is
    /// issued from here; the only host contacted is the target (plus the
    /// public DNS resolvers the `dns-aid` check queries, which are
    /// withheld for a local target unless `--external-dns` is set).
    #[command(after_help = "Examples:
  anc web localhost:8787                       # a local dev server (scheme defaults to http)
  anc web https://anc.dev                      # a public site
  anc web staging.internal:8080 --verbose      # list every passing row too
  anc web anc.dev --output json                # the scorecard on stdout, nothing else
  anc web anc.dev --check llms-txt             # gate a script on one check
  anc web anc.dev --site-type api              # score only the checks an API answers for
  anc emit web-checks                          # every check id, offline
  anc emit web-remediation                     # the whole fix catalog, offline

When the first argument looks like a network target, `web` is inserted
automatically:
  anc anc.dev        ≡  anc web anc.dev
  anc localhost:8787 ≡  anc web localhost:8787
A path that exists wins, so `anc ./anc.dev` still audits the directory.

Exit codes (shared with `anc audit`):
  0  clean: every applicable check passed
  1  warnings only: a SHOULD or MAY check missed
  2  failures present: a MUST check missed, or a usage error
  3  could not check: the target was unreachable, a probe errored or was cut
     short by the deadline, or every selected check was inapplicable")]
    Web {
        /// Target URL or host. A scheme-less target defaults to https,
        /// except localhost and local IP literals, which default to http.
        #[arg(value_hint = ValueHint::Url)]
        target: String,

        /// Score only the checks that apply to this kind of site. Omitted,
        /// every check applies, matching the hosted auditor's default.
        #[arg(long, value_name = "KIND")]
        site_type: Option<SiteType>,

        /// Report one check by id and exit with that row's code. Run
        /// `anc emit web-checks` for the full list of ids.
        #[arg(long, value_name = "ID")]
        check: Option<String>,

        /// Let the DNS-over-HTTPS checks query public resolvers even when
        /// the target is local or private. Without it those rows report
        /// `n_a` rather than sending the target's hostname to a third
        /// party.
        #[arg(long)]
        external_dns: bool,

        /// Output format
        #[arg(long, default_value = "text")]
        output: OutputFormat,
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
  anc emit schema | jq '.title'                       # pipe into jq for inspection
  anc emit web-checks | jq -r '.checks[].id'          # every `anc web --check` id
  anc emit web-schema                                 # the web-scorecard JSON Schema
  anc emit web-remediation                            # the web-audit fix catalog")]
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
    /// git clone --depth 1 https://github.com/brettdavies/agentnative-skill.git <dest>
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
    /// Render the spec coverage matrix (registry → audits → artifact).
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
    /// Print the compiled web-audit check registry as JSON.
    ///
    /// Every id `anc web --check <id>` accepts, with the label, category,
    /// tier, keyword, principle, site types and hint the run scores it
    /// against. Compiled into the binary from the vendored anc.dev
    /// registry, so it needs no network.
    WebChecks,
    /// Print the web-scorecard JSON Schema (draft 2020-12) to stdout.
    ///
    /// The contract `anc web --output json` emits, which is the same
    /// document the hosted auditor's scorecards validate against.
    WebSchema,
    /// Print the web-audit fix catalog as JSON.
    ///
    /// One entry per check: the goal, the markdown fix and its doc links,
    /// the same text `anc web` prints under a failing row. Compiled in, so
    /// an agent can read the whole catalog offline.
    WebRemediation,
}

/// `--site-type content|api` — which family of checks applies. Mirrors the
/// hosted auditor's caller-supplied `site_type`, which is never detected:
/// omitting it applies every check.
#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
#[value(rename_all = "kebab-case")]
pub enum SiteType {
    /// A documentation or content site.
    Content,
    /// An API.
    Api,
}

impl From<SiteType> for crate::web_audit::scorecard::DeclaredSiteType {
    fn from(t: SiteType) -> Self {
        match t {
            SiteType::Content => crate::web_audit::scorecard::DeclaredSiteType::Content,
            SiteType::Api => crate::web_audit::scorecard::DeclaredSiteType::Api,
        }
    }
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
    /// relaxations as those audits land.
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
