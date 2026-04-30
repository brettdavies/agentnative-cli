mod argv;
mod build_info;
mod check;
mod checks;
mod cli;
mod error;
mod principles;
mod project;
mod runner;
mod scorecard;
mod skill_install;
mod source;
mod types;

use std::time::{Duration, Instant};

use clap::Parser as _;
use clap_complete::generate;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use argv::{format_invocation, inject_default_subcommand};
use build_info::{ANC_COMMIT, ANC_VERSION};
use check::Check;
use checks::behavioral::all_behavioral_checks;
use checks::project::all_project_checks;
use checks::source::all_source_checks;
use cli::{Cli, Commands, GenerateKind, OutputFormat, SkillCmd};
use error::AppError;
use principles::matrix;
use principles::registry::{ExceptionCategory, SUPPRESSION_EVIDENCE_PREFIX, suppresses};
use project::Project;
use runner::{BinaryRunner, RunStatus};
use scorecard::{
    AncInfo, PlatformInfo, RunInfo, RunMetadata, TargetInfo, ToolInfo, audience, compute_badge,
    exit_code, format_json, format_text,
};
use types::{CheckGroup, CheckResult, CheckStatus, Confidence};

fn main() {
    // Fix SIGPIPE handling so piping to head/grep works correctly.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let code = match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            2
        }
    };
    std::process::exit(code);
}

fn run() -> Result<i32, AppError> {
    // Capture argv *before* `inject_default_subcommand` rewrites bare paths
    // into `check <path>`, so the scorecard's `run.invocation` reflects what
    // the user actually typed (R4). The injection rewrite is an internal
    // detail; recording it would lie about user intent.
    let raw_argv: Vec<std::ffi::OsString> = std::env::args_os().collect();

    let cli = Cli::parse_from(inject_default_subcommand(raw_argv.iter().cloned()));

    // --quiet is global (visible in top-level --help for agent discoverability)
    let quiet = cli.quiet;

    // Bare invocation (no args at all) is handled by clap's arg_required_else_help.
    // A flag-only invocation like `anc -q` parses successfully with `command =
    // None` — render help to stderr and exit 2 to mirror clap's contract.
    let (path, command, binary_only, source_only, principle, output, include_tests, audit_profile) =
        match cli.command {
            Some(Commands::Check {
                path,
                command,
                binary,
                source,
                principle,
                output,
                include_tests,
                audit_profile,
            }) => (
                path,
                command,
                binary,
                source,
                principle,
                output,
                include_tests,
                audit_profile,
            ),
            Some(Commands::Completions { shell }) => {
                let mut cmd = <Cli as clap::CommandFactory>::command();
                generate(shell, &mut cmd, "anc", &mut std::io::stdout());
                return Ok(0);
            }
            Some(Commands::Generate { artifact }) => {
                return run_generate(artifact);
            }
            Some(Commands::Skill { cmd }) => {
                return run_skill(cmd);
            }
            None => {
                let mut cmd = <Cli as clap::CommandFactory>::command();
                eprintln!("{}", cmd.render_help());
                return Ok(2);
            }
        };

    // Run-level timing starts at the top of the Check arm (R4): wall-clock
    // milliseconds and an RFC 3339 UTC timestamp. We use `OffsetDateTime` for
    // formatting only — duration math goes through `Instant` which is
    // monotonic and unaffected by wall-clock adjustments.
    let start_instant = Instant::now();
    let started_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z"));

    // --command resolves a binary from PATH and runs behavioral checks against
    // it. conflicts_with = "path" ensures only one of the two is provided.
    let command_name = command.clone();
    let resolved_path = match command {
        Some(name) => resolve_command_on_path(&name)?,
        None => path,
    };

    let mut project = Project::discover(&resolved_path)?;
    project.include_tests = include_tests;

    // Collect applicable checks based on flags and auto-detection
    let mut all_checks: Vec<Box<dyn Check>> = Vec::new();

    let has_binary = project.runner.is_some();
    let has_language = project.language.is_some();

    if !source_only {
        if has_binary {
            all_checks.extend(all_behavioral_checks());
        } else if binary_only {
            eprintln!("warning: --binary specified but no binary found");
        } else if has_language {
            eprintln!("warning: no binary found, running source checks only");
        }
    }

    if !binary_only {
        if let Some(lang) = project.language {
            all_checks.extend(all_source_checks(lang));
        } else if source_only {
            eprintln!("warning: --source specified but no language detected");
        }
    }

    // Project checks — always collected when path is a directory and not binary-only
    if !binary_only && project.path.is_dir() {
        all_checks.extend(all_project_checks());
    }

    // Translate the CLI-facing AuditProfile into the registry's
    // ExceptionCategory. Kept local so the registry stays CLI-agnostic.
    let exception_category: Option<ExceptionCategory> = audit_profile.map(Into::into);

    // Run checks. When an audit_profile is set, checks whose IDs appear in
    // the suppression table short-circuit to Skip with structured evidence
    // — they still appear in `results[]` so the scorecard shows what was
    // excluded and why.
    let mut results: Vec<CheckResult> = Vec::new();
    for check in &all_checks {
        if !check.applicable(&project) {
            continue;
        }
        if let Some(cat) = exception_category
            && suppresses(check.id(), cat)
        {
            results.push(CheckResult {
                id: check.id().to_string(),
                label: check.label().to_string(),
                group: check.group(),
                layer: check.layer(),
                status: CheckStatus::Skip(format!(
                    "{SUPPRESSION_EVIDENCE_PREFIX}{}",
                    cat.as_kebab_case()
                )),
                confidence: Confidence::High,
            });
            continue;
        }
        let result = match check.run(&project) {
            Ok(r) => r,
            Err(e) => CheckResult {
                id: check.id().to_string(),
                label: check.label().to_string(),
                group: check.group(),
                layer: check.layer(),
                status: CheckStatus::Error(e.to_string()),
                confidence: Confidence::High,
            },
        };
        results.push(result);
    }

    // Filter by principle number
    if let Some(p) = principle {
        results.retain(|r| matches_principle(&r.group, p));
    }

    // Compute audience from the 4 signal checks. Read-only over results;
    // Returns None when any signal check is missing from the vector — the
    // suppression loop above is the usual reason signal checks drop out
    // (e.g., human-tui suppresses `p1-non-interactive`).
    let audience_label = audience::classify(&results);
    let audit_profile_label = exception_category.map(|c| c.as_kebab_case().to_string());

    // Format output. `format_json` needs the check catalog so it can map
    // result IDs back to the requirements each check covers, plus the
    // run-level metadata (`tool`, `anc`, `run`, `target`). For text mode
    // we still need the tool slug so the badge hint can render the
    // canonical embed URL — derive it cheaply (no version probe) and
    // hand it to `compute_badge`.
    let output_str = match output {
        OutputFormat::Text => {
            let tool_name = derive_tool_name(command_name.as_deref(), &project);
            let badge = compute_badge(&results, &tool_name);
            format_text(&results, quiet, Some(&badge))
        }
        OutputFormat::Json => {
            let target = build_target_info(command_name.as_deref(), &project);
            let tool = build_tool_info(command_name.as_deref(), &project);
            let invocation = format_invocation(&raw_argv);
            let duration_ms =
                u64::try_from(start_instant.elapsed().as_millis()).unwrap_or(u64::MAX);
            let metadata = RunMetadata {
                tool,
                anc: AncInfo {
                    version: ANC_VERSION,
                    commit: ANC_COMMIT,
                },
                run: RunInfo {
                    invocation,
                    started_at,
                    duration_ms,
                    platform: PlatformInfo {
                        os: std::env::consts::OS,
                        arch: std::env::consts::ARCH,
                    },
                },
                target,
            };
            format_json(
                &results,
                &all_checks,
                audience_label,
                audit_profile_label,
                metadata,
            )
        }
    };
    print!("{output_str}");

    Ok(exit_code(&results))
}

/// Classify what `anc check` was pointed at into structured `target` metadata.
/// Three modes: `command` (PATH-resolved), `binary` (file argument), `project`
/// (directory argument).
fn build_target_info(command_name: Option<&str>, project: &Project) -> TargetInfo {
    match command_name {
        Some(name) => TargetInfo {
            kind: "command".into(),
            path: None,
            command: Some(name.to_string()),
        },
        None if project.path.is_dir() => TargetInfo {
            kind: "project".into(),
            path: Some(project.path.to_string_lossy().into_owned()),
            command: None,
        },
        None => TargetInfo {
            kind: "binary".into(),
            path: Some(project.path.to_string_lossy().into_owned()),
            command: None,
        },
    }
}

/// Cheap slug derivation: the same `name` `build_tool_info` would emit, but
/// without the manifest read or `--version` subprocess probe. Used by the
/// text-mode badge hint, where we need the slug to render the embed URL but
/// have no use for the version. Keeping this in lock-step with
/// `build_tool_info`'s `name` calculation guarantees the text-mode hint
/// references the same `<tool>` slug a `--output json` consumer would see.
fn derive_tool_name(command_name: Option<&str>, project: &Project) -> String {
    match command_name {
        Some(cmd) => cmd.to_string(),
        None => project
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .map(String::from)
            .unwrap_or_default(),
    }
}

/// Build the scorecard's `tool` block. `name` is always present (deterministic
/// from path / command name). `binary` is the executable basename when one
/// exists. `version` is best-effort: project-mode prefers the manifest version,
/// command/binary mode probes `<bin> --version` / `<bin> -V`. Any failure
/// yields `null` rather than aborting the run.
fn build_tool_info(command_name: Option<&str>, project: &Project) -> ToolInfo {
    let name = derive_tool_name(command_name, project);
    let (binary, version_seed) = match command_name {
        Some(cmd) => {
            // Command mode: binary echoes the user-supplied name (NOT the
            // resolved path — we don't want to leak /usr/local/bin/foo as
            // the binary identifier).
            (Some(cmd.to_string()), None)
        }
        None => {
            if project.path.is_dir() {
                let manifest_version = project
                    .manifest_path
                    .as_deref()
                    .and_then(read_manifest_version);
                let binary_name = project
                    .binary_paths
                    .first()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .map(String::from);
                (binary_name, manifest_version)
            } else {
                // Binary file passed directly.
                (Some(name.clone()), None)
            }
        }
    };

    // Manifest version takes precedence; fall back to the binary self-report.
    let version = version_seed.or_else(|| probe_tool_version(project));

    ToolInfo {
        name,
        binary,
        version,
    }
}

/// Best-effort `<binary> --version` / `<binary> -V` probe. Reuses the runner's
/// timeout + 1MB cap primitives via a fresh `BinaryRunner` with a tighter
/// 2-second timeout (the version probe is one-shot, not a check).
///
/// Self-spawn guard: comparing the resolved binary path to `current_exe()`
/// declines the probe when `anc` is asked to score itself. Without this,
/// `anc check --command anc` would recursively score `anc` — bounded only by
/// `arg_required_else_help` in `Cli`. Belt-and-suspenders.
fn probe_tool_version(project: &Project) -> Option<String> {
    let binary = project.binary_paths.first()?;

    if let Ok(self_exe) = std::env::current_exe()
        && let (Ok(a), Ok(b)) = (binary.canonicalize(), self_exe.canonicalize())
        && a == b
    {
        // Both paths canonicalized (Project::discover canonicalizes; the OS
        // resolves current_exe). Direct comparison is the right primitive.
        return None;
    }

    let runner = BinaryRunner::new(binary.clone(), Duration::from_secs(2)).ok()?;
    for flag in ["--version", "-V"] {
        let result = runner.run(&[flag], &[]);
        if matches!(result.status, RunStatus::Ok)
            && result.exit_code == Some(0)
            && let Some(line) = result.stdout.lines().next()
        {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Read `package.version` from a Cargo.toml or `project.version` from a
/// pyproject.toml. Returns `None` for unreadable / unparseable / missing-field
/// cases — the version probe falls through to the binary self-report.
fn read_manifest_version(manifest: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(manifest).ok()?;
    let parsed: toml::Value = content.parse().ok()?;

    // Cargo.toml: [package] version = "...".
    if let Some(v) = parsed
        .get("package")
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
    {
        return Some(v.to_string());
    }
    // pyproject.toml: [project] version = "...".
    if let Some(v) = parsed
        .get("project")
        .and_then(|p| p.get("version"))
        .and_then(|v| v.as_str())
    {
        return Some(v.to_string());
    }
    None
}

/// Resolve a command name to an absolute path by shelling out to `which`
/// (Unix) or `where` (Windows). Returns a clear, actionable error when the
/// name cannot be found on PATH. Subsequent `Project::discover()` validates
/// that the resolved path is an executable file.
fn resolve_command_on_path(name: &str) -> Result<std::path::PathBuf, AppError> {
    let locator = if cfg!(windows) { "where" } else { "which" };

    let output = std::process::Command::new(locator)
        .arg(name)
        .output()
        .map_err(|e| {
            AppError::ProjectDetection(anyhow::anyhow!("failed to invoke `{locator}`: {e}"))
        })?;

    if !output.status.success() {
        return Err(AppError::ProjectDetection(anyhow::anyhow!(
            "command '{name}' not found on PATH"
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    // `which` and `where` both print one match per line; take the first.
    let first = stdout.lines().next().map(str::trim).unwrap_or("");
    if first.is_empty() {
        return Err(AppError::ProjectDetection(anyhow::anyhow!(
            "command '{name}' not found on PATH"
        )));
    }

    Ok(std::path::PathBuf::from(first))
}

fn run_skill(cmd: SkillCmd) -> Result<i32, AppError> {
    match cmd {
        SkillCmd::Install {
            host,
            dry_run,
            output,
        } => skill_install::run_install(host, dry_run, output),
    }
}

fn run_generate(artifact: GenerateKind) -> Result<i32, AppError> {
    match artifact {
        GenerateKind::CoverageMatrix {
            out,
            json_out,
            check,
        } => {
            let catalog = checks::all_checks_catalog();

            // Dangling `covers()` references are a registry bug — surface
            // them before writing artifacts so CI catches the regression
            // at `generate --check` time too.
            let dangling = matrix::dangling_cover_ids(&catalog);
            if !dangling.is_empty() {
                for (check_id, req_id) in &dangling {
                    eprintln!("error: check `{check_id}` covers unknown requirement `{req_id}`");
                }
                return Err(AppError::ProjectDetection(anyhow::anyhow!(
                    "registry drift: {} dangling requirement reference(s)",
                    dangling.len()
                )));
            }

            let m = matrix::build(&catalog);
            let rendered_md = matrix::render_markdown(&m);
            let rendered_json = matrix::render_json(&m);

            if check {
                // Drift mode: compare generated output to committed artifacts.
                // Fail with actionable evidence so CI points the operator at
                // `anc generate coverage-matrix` as the fix.
                let existing_md = std::fs::read_to_string(&out).unwrap_or_default();
                let existing_json = std::fs::read_to_string(&json_out).unwrap_or_default();
                let md_matches = normalize_trailing_newline(&existing_md)
                    == normalize_trailing_newline(&rendered_md);
                let json_matches = normalize_trailing_newline(&existing_json)
                    == normalize_trailing_newline(&rendered_json);
                if !md_matches {
                    eprintln!(
                        "error: {} is out of date — run `anc generate coverage-matrix`",
                        out.display()
                    );
                }
                if !json_matches {
                    eprintln!(
                        "error: {} is out of date — run `anc generate coverage-matrix`",
                        json_out.display()
                    );
                }
                return Ok(if md_matches && json_matches { 0 } else { 2 });
            }

            if let Some(parent) = out.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AppError::ProjectDetection(anyhow::anyhow!(
                        "creating parent dir for {}: {e}",
                        out.display()
                    ))
                })?;
            }
            if let Some(parent) = json_out.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AppError::ProjectDetection(anyhow::anyhow!(
                        "creating parent dir for {}: {e}",
                        json_out.display()
                    ))
                })?;
            }
            std::fs::write(&out, &rendered_md).map_err(|e| {
                AppError::ProjectDetection(anyhow::anyhow!("writing {}: {e}", out.display()))
            })?;
            std::fs::write(&json_out, &rendered_json).map_err(|e| {
                AppError::ProjectDetection(anyhow::anyhow!("writing {}: {e}", json_out.display()))
            })?;
            eprintln!(
                "wrote {} ({} rows) and {}",
                out.display(),
                m.rows.len(),
                json_out.display()
            );
            Ok(0)
        }
    }
}

fn normalize_trailing_newline(s: &str) -> &str {
    s.trim_end_matches('\n')
}

fn matches_principle(group: &CheckGroup, principle: u8) -> bool {
    // CodeQuality and ProjectStructure checks are cross-cutting — always include them.
    matches!(
        group,
        CheckGroup::CodeQuality | CheckGroup::ProjectStructure
    ) || matches!(
        (group, principle),
        (CheckGroup::P1, 1)
            | (CheckGroup::P2, 2)
            | (CheckGroup::P3, 3)
            | (CheckGroup::P4, 4)
            | (CheckGroup::P5, 5)
            | (CheckGroup::P6, 6)
            | (CheckGroup::P7, 7)
    )
}
