//! End-to-end coverage for the `p6-may-standard-names` audit's `.anc.toml`
//! integration. Exercises the loader + audit through the real `anc` binary
//! so the parse-error -> Warn path and the domain-verb -> Pass path are
//! verified against the published surface.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use assert_cmd::Command;
use serde_json::Value;

mod common;

/// Build a Command for the anc binary.
fn cmd() -> Command {
    let mut cmd = Command::cargo_bin("anc").expect("binary should exist");
    cmd.env("AGENTNATIVE_HOME_CONFIG", common::empty_home_config());
    cmd
}

/// Allocate a unique tempdir for one test. Avoids cross-test collision when
/// the cargo test runner schedules these in parallel.
fn unique_tempdir(label: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let id = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "anc-standard-names-{label}-{}-{id}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after epoch")
            .as_nanos(),
    ));
    fs::create_dir_all(&dir).expect("create tempdir");
    dir
}

/// Shell fixture exposing three subcommands (`archive`, `follow`, `mentions`).
/// `archive` and `follow` are cross-domain built-in standard verbs that
/// survived the platform-verb trim (2/3 = 0.67, below the 0.70 pass
/// threshold); `mentions` is X-specific and must come from `.anc.toml
/// [p6] domain_verbs` to push the ratio across the bar (3/3 = 1.0). `help`
/// is intentionally omitted from the help block — clap always emits it,
/// but including it would add a fourth standard verb and let the fixture
/// pass without exercising the loader at all.
const FIXTURE_COMMANDS: &[&str] = &["archive", "follow", "mentions"];

/// A shell CLI whose `--help` lists `commands` as its subcommands.
fn fixture_script(commands: &[&str]) -> String {
    let listing: String = commands
        .iter()
        .map(|name| format!("  {name:<10} Run {name}\n"))
        .collect();
    format!(
        r#"#!/bin/sh
case "$1" in
  --help) cat <<'EOF'
Usage: x [OPTIONS] <COMMAND>

Commands:
{listing}
Options:
  -h, --help     Show help
  -V, --version  Print version
EOF
    exit 0 ;;
  --version) echo "x 0.1.0"; exit 0 ;;
  *) echo "x tool"; exit 0 ;;
esac
"#
    )
}

/// Write the fixture CLI to `path` as an executable.
fn write_fixture(path: &Path, commands: &[&str]) {
    fs::create_dir_all(path.parent().expect("fixture has a parent")).expect("mkdir fixture dir");
    let script = fixture_script(commands);
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o755)
            .open(path)
            .expect("open fixture binary");
        f.write_all(script.as_bytes())
            .expect("write fixture binary");
    }
    #[cfg(not(unix))]
    {
        fs::write(path, script).expect("write fixture binary");
    }
}

/// Stage `dir` as a Python project whose `pyproject.toml` declares the
/// script `x`, built as the fixture binary at `.venv/bin/x`.
fn stage_project(dir: &std::path::Path) -> PathBuf {
    stage_project_with(dir, FIXTURE_COMMANDS)
}

fn stage_project_with(dir: &Path, commands: &[&str]) -> PathBuf {
    fs::write(
        dir.join("pyproject.toml"),
        "[project]\nname = \"x\"\nversion = \"0.1.0\"\n\n[project.scripts]\nx = \"x:main\"\n",
    )
    .expect("write pyproject.toml");
    let bin = dir.join(".venv").join("bin").join("x");
    write_fixture(&bin, commands);
    bin
}

/// A checkout: a `.git` directory, a root `.anc.toml` declaring `mentions`,
/// and the fixture built into `target/release/<name>`. Returns the binary.
fn stage_checkout(label: &str, name: &str) -> PathBuf {
    let root = unique_tempdir(label);
    fs::create_dir_all(root.join(".git")).expect("mkdir .git");
    fs::write(
        root.join(".anc.toml"),
        "[p6]\ndomain_verbs = [\"mentions\"]\n",
    )
    .expect("write .anc.toml");
    let bin = root.join("target").join("release").join(name);
    write_fixture(&bin, FIXTURE_COMMANDS);
    bin
}

/// Run `anc audit <bin> --output json` and pluck the audit row for
/// `p6-may-standard-names`. Returns `(status, evidence)` where evidence is
/// the empty string when absent (Pass rows carry no evidence).
fn run_audit_and_extract(target: &std::path::Path) -> (String, String) {
    let row = p6_row(cmd().args([
        "audit",
        target.to_str().expect("utf8 path"),
        "--output",
        "json",
    ]));

    let status = row["status"]
        .as_str()
        .expect("status is a string")
        .to_string();
    let evidence = row["evidence"].as_str().unwrap_or("").to_string();
    (status, evidence)
}

/// Run a prepared `anc audit ... --output json` and return the
/// `p6-may-standard-names` row.
fn p6_row(cmd: &mut Command) -> Value {
    let output = cmd.output().expect("spawn anc");
    let json_str = String::from_utf8(output.stdout).expect("stdout valid UTF-8");
    let parsed: Value = serde_json::from_str(&json_str)
        .unwrap_or_else(|e| panic!("scorecard is valid JSON ({e}); stdout: {json_str}"));

    let results = parsed["results"]
        .as_array()
        .expect("scorecard.results is an array");

    results
        .iter()
        .find(|r| r["id"].as_str() == Some("p6-may-standard-names"))
        .expect("scorecard contains p6-may-standard-names row")
        .clone()
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("utf8 path")
}

fn assert_passes_with_domain_verbs(row: &Value) {
    assert_eq!(row["status"], "pass", "row: {row}");
    assert_eq!(row["using_domain_verbs"], true, "row: {row}");
}

#[test]
fn standard_names_passes_with_anc_toml_domain_verbs() {
    let dir = unique_tempdir("pass-with-domain-verbs");
    let bin = stage_project(&dir);
    fs::write(
        dir.join(".anc.toml"),
        "[p6]\ndomain_verbs = [\"mentions\"]\n",
    )
    .expect("write .anc.toml");

    let (status, _evidence) = run_audit_and_extract(&dir);
    assert_eq!(
        status,
        "pass",
        "expected pass once domain_verbs covers `mentions`, got status `{status}` (bin: {})",
        bin.display()
    );
}

#[test]
fn standard_names_warns_when_anc_toml_malformed() {
    let dir = unique_tempdir("warn-on-malformed");
    let _bin = stage_project(&dir);
    fs::write(dir.join(".anc.toml"), "[p6]\ndomain_verbs = \"post\"\n")
        .expect("write malformed .anc.toml");

    let (status, evidence) = run_audit_and_extract(&dir);
    assert_eq!(
        status, "warn",
        "expected warn when .anc.toml fails to parse, got `{status}`"
    );
    assert!(
        evidence.contains("could not parse .anc.toml"),
        "expected parse-error evidence, got: {evidence}"
    );
}

#[test]
fn standard_names_no_op_when_anc_toml_absent() {
    // Regression: without `.anc.toml`, the fixture still warns because
    // `mentions` isn't in the built-in list. Locks the additive-only
    // contract — absent config never silently adds vocabulary.
    let dir = unique_tempdir("warn-when-absent");
    let _bin = stage_project(&dir);

    let (status, evidence) = run_audit_and_extract(&dir);
    assert_eq!(
        status, "warn",
        "expected warn without .anc.toml, got `{status}`"
    );
    assert!(
        evidence.contains("mentions"),
        "expected `mentions` in non-standard evidence list, got: {evidence}"
    );
}

#[test]
fn binary_in_a_checkout_reads_the_root_config() {
    let bin = stage_checkout("checkout-binary", "x");

    let row = p6_row(cmd().args(["audit", path_str(&bin), "--output", "json"]));

    assert_passes_with_domain_verbs(&row);
}

#[test]
#[cfg(unix)]
fn symlink_outside_the_checkout_reads_the_real_files_config() {
    let bin = stage_checkout("checkout-symlink", "x");
    let link = unique_tempdir("symlink-dir").join("x");
    std::os::unix::fs::symlink(&bin, &link).expect("symlink fixture");

    let row = p6_row(cmd().args(["audit", path_str(&link), "--output", "json"]));

    assert_passes_with_domain_verbs(&row);
}

#[test]
fn command_resolving_into_a_checkout_reads_its_config() {
    let bin = stage_checkout("checkout-command", "anc-standard-names-fixture");
    let bin_dir = bin.parent().expect("bin dir").to_path_buf();
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let path =
        std::env::join_paths(std::iter::once(bin_dir).chain(std::env::split_paths(&inherited)))
            .expect("join PATH");

    let row = p6_row(cmd().env("PATH", path).args([
        "audit",
        "--command",
        "anc-standard-names-fixture",
        "--output",
        "json",
    ]));

    assert_passes_with_domain_verbs(&row);
}

#[test]
fn subdirectory_target_merges_root_and_nested_configs() {
    let root = unique_tempdir("nested");
    fs::create_dir_all(root.join(".git")).expect("mkdir .git");
    fs::write(
        root.join(".anc.toml"),
        "[p6]\ndomain_verbs = [\"mentions\"]\n",
    )
    .expect("write root .anc.toml");
    let cli = root.join("crates").join("cli");
    fs::create_dir_all(&cli).expect("mkdir crates/cli");
    stage_project_with(&cli, &["archive", "mentions", "timeline"]);
    fs::write(
        cli.join(".anc.toml"),
        "[p6]\ndomain_verbs = [\"timeline\"]\n",
    )
    .expect("write nested .anc.toml");

    let row = p6_row(cmd().args(["audit", path_str(&cli), "--output", "json"]));

    assert_passes_with_domain_verbs(&row);
    assert_eq!(row["domain_match_count"], 2, "row: {row}");
}

/// A binary outside any repository, and a separate directory holding a
/// fetched `.anc.toml` that declares `mentions`.
fn stage_fetched_repo(label: &str) -> (PathBuf, PathBuf) {
    let bin = unique_tempdir(&format!("{label}-bin")).join("x");
    write_fixture(&bin, FIXTURE_COMMANDS);
    let fetched = unique_tempdir(&format!("{label}-fetched"));
    fs::write(
        fetched.join(".anc.toml"),
        "[p6]\ndomain_verbs = [\"mentions\"]\n",
    )
    .expect("write fetched .anc.toml");
    (bin, fetched)
}

#[test]
fn repo_flag_supplies_the_config_for_a_binary_outside_any_repo() {
    let (bin, fetched) = stage_fetched_repo("repo-flag");

    let row = p6_row(cmd().args([
        "audit",
        path_str(&bin),
        "--repo",
        path_str(&fetched),
        "--output",
        "json",
    ]));

    assert_passes_with_domain_verbs(&row);
}

#[test]
fn repo_env_var_works_like_the_flag() {
    let (bin, fetched) = stage_fetched_repo("repo-env");

    let row = p6_row(cmd().env("AGENTNATIVE_REPO", &fetched).args([
        "audit",
        path_str(&bin),
        "--output",
        "json",
    ]));

    assert_passes_with_domain_verbs(&row);
}

/// Run `anc audit <bin> --repo <repo> --output json`, expect a usage error,
/// and return its JSON envelope.
fn repo_usage_error(repo: &Path) -> Value {
    let (bin, _) = stage_fetched_repo("repo-usage");
    let output = cmd()
        .args([
            "audit",
            path_str(&bin),
            "--repo",
            path_str(repo),
            "--output",
            "json",
        ])
        .output()
        .expect("spawn anc");
    assert_eq!(output.status.code(), Some(2), "output: {output:?}");
    let stderr = String::from_utf8(output.stderr).expect("stderr valid UTF-8");
    let envelope: Value = serde_json::from_str(stderr.trim())
        .unwrap_or_else(|e| panic!("usage envelope is JSON ({e}); stderr: {stderr}"));
    assert_eq!(envelope["kind"], "usage", "envelope: {envelope}");
    assert_eq!(envelope["exit_code"], 2, "envelope: {envelope}");
    envelope
}

#[test]
fn repo_flag_rejects_a_missing_directory() {
    let missing = unique_tempdir("repo-missing").join("absent");

    let envelope = repo_usage_error(&missing);

    let message = envelope["message"].as_str().expect("message");
    assert!(
        message.contains("no such directory"),
        "envelope: {envelope}"
    );
}

#[test]
fn repo_flag_rejects_a_regular_file() {
    let file = unique_tempdir("repo-file").join("notes.txt");
    fs::write(&file, "not a repo\n").expect("write file");

    let envelope = repo_usage_error(&file);

    let message = envelope["message"].as_str().expect("message");
    assert!(message.contains("not a directory"), "envelope: {envelope}");
}

#[test]
fn home_config_applies_to_a_target_outside_any_repo() {
    let home = unique_tempdir("home");
    fs::write(
        home.join(".anc.toml"),
        "[p6]\ndomain_verbs = [\"mentions\"]\n",
    )
    .expect("write home .anc.toml");
    let bin = unique_tempdir("home-target").join("x");
    write_fixture(&bin, FIXTURE_COMMANDS);

    let row = p6_row(
        cmd()
            .env("AGENTNATIVE_HOME_CONFIG", home.join(".anc.toml"))
            .args(["audit", path_str(&bin), "--output", "json"]),
    );

    assert_passes_with_domain_verbs(&row);
}

#[test]
fn relocated_home_config_is_named_by_its_variable() {
    let home = unique_tempdir("relocated-home");
    fs::write(home.join("anc.toml"), "[p6]\ndomain_verbs = [\"post\"\n")
        .expect("write broken home config");
    let bin = unique_tempdir("relocated-home-target").join("x");
    write_fixture(&bin, FIXTURE_COMMANDS);

    let row = p6_row(
        cmd()
            .env("AGENTNATIVE_HOME_CONFIG", home.join("anc.toml"))
            .args(["audit", path_str(&bin), "--output", "json"]),
    );

    let evidence = row["evidence"].as_str().expect("evidence");
    assert!(
        evidence.starts_with("could not parse .anc.toml at $AGENTNATIVE_HOME_CONFIG:"),
        "the variable that placed the file names it: {evidence}"
    );
    assert!(!evidence.contains("~/.anc.toml"), "evidence: {evidence}");
}

#[test]
fn a_relocated_home_config_that_does_not_exist_warns() {
    let missing = unique_tempdir("missing-home").join("typo.toml");
    let bin = unique_tempdir("missing-home-target").join("x");
    write_fixture(&bin, FIXTURE_COMMANDS);

    let output = cmd()
        .env("AGENTNATIVE_HOME_CONFIG", &missing)
        .args(["audit", path_str(&bin), "--output", "json"])
        .output()
        .expect("spawn anc");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = format!(
        "warning: AGENTNATIVE_HOME_CONFIG names {}, which does not exist; no user-level .anc.toml applies",
        missing.display()
    );
    assert!(stderr.contains(&expected), "stderr: {stderr}");
    let scorecard: Value = serde_json::from_slice(&output.stdout).expect("scorecard JSON");
    assert!(scorecard["results"].is_array(), "the audit still runs");
}

const README_CONFIG_SECTION: &str =
    "https://github.com/brettdavies/agentnative-cli#configuration-anctoml";

/// Run a prepared `anc audit ... --output json` and return the scorecard.
fn scorecard(cmd: &mut Command) -> Value {
    let output = cmd.output().expect("spawn anc");
    serde_json::from_slice(&output.stdout).expect("scorecard is valid JSON")
}

/// The fixture binary alone in a directory outside any repository.
fn lone_fixture(label: &str, commands: &[&str]) -> PathBuf {
    let bin = unique_tempdir(label).join("x");
    write_fixture(&bin, commands);
    bin
}

#[test]
fn warning_without_config_hints_the_line_that_clears_it() {
    let bin = lone_fixture("hint-json", FIXTURE_COMMANDS);

    let row = p6_row(cmd().args(["audit", path_str(&bin), "--output", "json"]));

    assert_eq!(row["status"], "warn", "row: {row}");
    let hint = &row["config_hint"];
    assert_eq!(
        hint["files"],
        serde_json::json!([
            {"file": ".anc.toml", "scope": "tool-repository"},
            {"file": "$AGENTNATIVE_HOME_CONFIG", "scope": "user"},
        ]),
        "a target outside any repository names the tool's repository file and the user file: {row}"
    );
    assert_eq!(
        hint["domain_verbs"],
        serde_json::json!(["mentions"]),
        "row: {row}"
    );
    assert_eq!(hint["docs"], README_CONFIG_SECTION, "row: {row}");
}

#[test]
fn warning_without_config_prints_the_hint_and_both_files_in_text_mode() {
    let bin = lone_fixture("hint-text", FIXTURE_COMMANDS);

    let output = cmd()
        .args(["audit", path_str(&bin)])
        .output()
        .expect("spawn anc");

    let stdout = String::from_utf8(output.stdout).expect("stdout valid UTF-8");
    let lines: Vec<&str> = stdout.lines().map(str::trim_start).collect();
    let at = lines
        .iter()
        .position(|line| line.starts_with("hint:"))
        .unwrap_or_else(|| panic!("no hint line: {stdout}"));
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with("hint:"))
            .count(),
        1,
        "stdout: {stdout}"
    );
    let hint = lines[at];
    assert!(
        hint.contains(r#"domain_verbs = ["mentions"]"#)
            && hint.contains("either file below")
            && hint.contains(README_CONFIG_SECTION),
        "hint: {hint}"
    );
    let repository = lines[at + 1];
    assert!(
        repository.starts_with("- .anc.toml at the tool's repository root:")
            && repository.contains("--repo <checkout>"),
        "repository line: {repository}"
    );
    let user = lines[at + 2];
    assert!(
        user.starts_with("- $AGENTNATIVE_HOME_CONFIG:") && user.contains("every tool you audit"),
        "user line: {user}"
    );
}

#[test]
fn binary_in_a_checkout_without_config_hints_the_repo_root_file() {
    let root = unique_tempdir("hint-checkout");
    fs::create_dir_all(root.join(".git")).expect("mkdir .git");
    let bin = root.join("target").join("release").join("x");
    write_fixture(&bin, FIXTURE_COMMANDS);

    let row = p6_row(cmd().args(["audit", path_str(&bin), "--output", "json"]));

    let files = &row["config_hint"]["files"];
    assert_eq!(
        files[0],
        serde_json::json!({"file": ".anc.toml", "scope": "repository"}),
        "row: {row}"
    );
    assert_eq!(files[1]["scope"], "user", "row: {row}");
}

#[test]
fn repo_flag_without_config_hints_that_directorys_file() {
    let bin = lone_fixture("hint-repo-bin", FIXTURE_COMMANDS);
    let fetched = unique_tempdir("hint-repo-fetched");

    let row = p6_row(cmd().args([
        "audit",
        path_str(&bin),
        "--repo",
        path_str(&fetched),
        "--output",
        "json",
    ]));

    assert_eq!(
        row["config_hint"]["files"][0],
        serde_json::json!({"file": ".anc.toml", "scope": "repository"}),
        "row: {row}"
    );
}

#[test]
fn config_that_still_misses_the_threshold_gets_no_hint() {
    let bin = lone_fixture("hint-short", &["archive", "mentions", "timeline", "zap"]);
    fs::write(
        bin.parent().expect("bin dir").join(".anc.toml"),
        "[p6]\ndomain_verbs = [\"timeline\"]\n",
    )
    .expect("write .anc.toml");

    let row = p6_row(cmd().args(["audit", path_str(&bin), "--output", "json"]));

    assert_eq!(row["status"], "warn", "row: {row}");
    assert!(row.get("config_hint").is_none(), "row: {row}");
}

#[test]
fn passing_row_gets_no_hint() {
    let bin = stage_checkout("hint-pass", "x");

    let row = p6_row(cmd().args(["audit", path_str(&bin), "--output", "json"]));

    assert_eq!(row["status"], "pass", "row: {row}");
    assert!(row.get("config_hint").is_none(), "row: {row}");
}

#[test]
fn invalid_chain_warns_without_a_hint() {
    let bin = lone_fixture("hint-invalid", FIXTURE_COMMANDS);
    let dir = bin.parent().expect("bin dir");
    fs::write(dir.join(".anc.toml"), "[p6\n").expect("write broken .anc.toml");

    let row = p6_row(cmd().args(["audit", path_str(&bin), "--output", "json"]));

    assert_eq!(row["status"], "warn", "row: {row}");
    let evidence = row["evidence"].as_str().expect("evidence");
    let named = format!(
        "could not parse .anc.toml at {}/.anc.toml",
        dir.file_name().expect("dir name").to_string_lossy()
    );
    assert!(evidence.starts_with(&named), "row: {row}");
    assert!(row.get("config_hint").is_none(), "row: {row}");
}

#[test]
fn only_the_standard_names_row_carries_a_hint_and_none_names_docs_solutions() {
    let bin = lone_fixture("hint-other-rows", FIXTURE_COMMANDS);

    let card = scorecard(cmd().args(["audit", path_str(&bin), "--output", "json"]));

    let rows = card["results"].as_array().expect("results array");
    let hinted: Vec<&str> = rows
        .iter()
        .filter(|row| row.get("config_hint").is_some())
        .filter_map(|row| row["id"].as_str())
        .collect();
    assert_eq!(hinted, ["p6-may-standard-names"], "scorecard: {card}");
    assert!(
        !card.to_string().contains("docs/solutions"),
        "scorecard: {card}"
    );
}
