//! End-to-end coverage for directory-target discovery: the package
//! inventory, binary selection, and what each run audits.

use std::fs;
use std::path::Path;

use assert_cmd::Command;

mod common;

fn cmd() -> Command {
    let mut cmd = Command::cargo_bin("anc").expect("anc binary should exist");
    cmd.env("AGENTNATIVE_HOME_CONFIG", common::empty_home_config());
    cmd
}

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, body).expect("write");
}

#[test]
fn an_unreadable_workspace_declaration_warns_on_stderr() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write(tmp.path(), "package.json", "{\"name\": \"app\"}\n");
    write(
        tmp.path(),
        "pnpm-workspace.yaml",
        "packages: &all\n  - 'pkgs/*'\n",
    );

    let output = cmd()
        .args(["audit", tmp.path().to_str().expect("utf-8 path")])
        .output()
        .expect("spawn anc");

    let stderr = String::from_utf8(output.stderr).expect("stderr valid UTF-8");
    let warning = stderr
        .lines()
        .find(|line| line.contains("pnpm-workspace.yaml"))
        .unwrap_or_else(|| panic!("no warning names pnpm-workspace.yaml; stderr: {stderr}"));
    assert!(warning.starts_with("warning: "), "stderr: {stderr}");
}

/// Write an executable stand-in CLI whose `--version` prints `version`.
#[cfg(unix)]
fn fixture_cli(path: &Path, version: &str) {
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\n  --help|-h) printf 'Usage: x [OPTIONS]\\n\\nOptions:\\n  -h, --help     Print help\\n  -V, --version  Print version\\n'; exit 0 ;;\n  --version|-V) echo \"x {version}\"; exit 0 ;;\n  *) exit 0 ;;\nesac\n"
    );
    write(
        path.parent().expect("parent"),
        path.file_name().expect("name").to_str().expect("utf-8"),
        &script,
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod");
    }
}

/// A Cargo workspace whose build produced two binaries: `xr` from
/// `crates/xr` in `target/release`, and `xdk-consumer-check` from
/// `crates/xdk` in `target/debug`.
#[cfg(unix)]
fn two_binary_workspace() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"3\"\n",
    );
    write(
        root,
        "crates/xr/Cargo.toml",
        "[package]\nname = \"xr\"\nversion = \"4.2.0\"\nedition = \"2024\"\n",
    );
    write(root, "crates/xr/src/main.rs", "fn main() {}\n");
    write(
        root,
        "crates/xdk/Cargo.toml",
        "[package]\nname = \"xdk\"\nversion = \"0.1.3\"\nedition = \"2024\"\n",
    );
    write(root, "crates/xdk/src/lib.rs", "");
    write(
        root,
        "crates/xdk/src/bin/xdk-consumer-check.rs",
        "fn main() {}\n",
    );
    fixture_cli(&root.join("target/release/xr"), "4.2.0");
    fixture_cli(&root.join("target/debug/xdk-consumer-check"), "0.1.3");
    tmp
}

#[cfg(unix)]
const BIN_SELECTION_DOCS: &str =
    "https://github.com/brettdavies/agentnative-cli#one-binary-several-or-none";

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr valid UTF-8")
}

fn json_of(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes)
        .unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(bytes)))
}

#[test]
#[cfg(unix)]
fn two_built_binaries_stop_the_run_with_one_command_each() {
    let tmp = two_binary_workspace();

    let output = cmd()
        .current_dir(tmp.path())
        .args(["audit", "."])
        .output()
        .expect("spawn anc");

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "no scorecard: {output:?}");
    let stderr = stderr_of(&output);
    for command in [
        "anc audit . --bin xr",
        "anc audit . --bin xdk-consumer-check",
    ] {
        let lines = stderr.lines().filter(|line| line.contains(command)).count();
        assert_eq!(lines, 1, "one line runs `{command}`; stderr: {stderr}");
    }
    assert!(
        stderr.contains("Usage: anc audit [OPTIONS] [PATH]"),
        "stderr: {stderr}"
    );
}

#[test]
#[cfg(unix)]
fn two_built_binaries_in_json_mode_name_each_candidate() {
    let tmp = two_binary_workspace();

    let output = cmd()
        .current_dir(tmp.path())
        .args(["audit", ".", "--output", "json"])
        .output()
        .expect("spawn anc");

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "no scorecard: {output:?}");
    let envelope = json_of(stderr_of(&output).trim().as_bytes());
    assert_eq!(envelope["kind"], "usage", "{envelope}");
    assert_eq!(envelope["error"], "binary-ambiguous", "{envelope}");
    assert_eq!(envelope["exit_code"], 2, "{envelope}");
    let candidates = envelope["candidates"].as_array().expect("candidates array");
    let xr = candidates
        .iter()
        .find(|c| c["name"] == "xr")
        .expect("xr is a candidate");
    assert_eq!(xr["package"], "xr", "{envelope}");
    assert_eq!(xr["path"], "target/release/xr", "{envelope}");
    assert_eq!(
        xr["command"], "anc audit . --output json --bin xr",
        "{envelope}"
    );
    assert!(
        candidates.iter().any(|c| c["name"] == "xdk-consumer-check"
            && c["package"] == "xdk"
            && c["path"] == "target/debug/xdk-consumer-check"
            && c["command"] == "anc audit . --output json --bin xdk-consumer-check"),
        "{envelope}"
    );
}

#[test]
#[cfg(unix)]
fn bin_flag_grades_the_named_candidate() {
    let tmp = two_binary_workspace();

    let output = cmd()
        .current_dir(tmp.path())
        .args(["audit", ".", "--bin", "xr", "--output", "json"])
        .output()
        .expect("spawn anc");

    let card = json_of(&output.stdout);
    assert_eq!(card["tool"]["binary"], "xr", "{card}");
    let behavioral = card["results"]
        .as_array()
        .expect("results")
        .iter()
        .filter(|row| row["layer"] == "behavioral")
        .count();
    assert!(behavioral > 0, "behavioral audits ran: {card}");
}

#[test]
#[cfg(unix)]
fn bin_env_var_works_like_the_flag() {
    let tmp = two_binary_workspace();

    let output = cmd()
        .current_dir(tmp.path())
        .env("AGENTNATIVE_BIN", "xr")
        .args(["audit", ".", "--output", "json"])
        .output()
        .expect("spawn anc");

    let card = json_of(&output.stdout);
    assert_eq!(card["tool"]["binary"], "xr", "{card}");
}

#[test]
#[cfg(unix)]
fn an_unknown_bin_lists_the_candidates() {
    let tmp = two_binary_workspace();

    let output = cmd()
        .current_dir(tmp.path())
        .args(["audit", ".", "--bin", "nope"])
        .output()
        .expect("spawn anc");

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = stderr_of(&output);
    assert!(stderr.contains("nope"), "stderr: {stderr}");
    assert!(stderr.contains("anc audit . --bin xr"), "stderr: {stderr}");
    assert!(
        stderr.contains("anc audit . --bin xdk-consumer-check"),
        "stderr: {stderr}"
    );
}

#[test]
#[cfg(unix)]
fn bin_beside_a_command_or_a_binary_path_is_a_usage_error() {
    let tmp = two_binary_workspace();

    let with_command = cmd()
        .args([
            "audit",
            "--command",
            "ls",
            "--bin",
            "xr",
            "--output",
            "json",
        ])
        .output()
        .expect("spawn anc");
    assert_eq!(with_command.status.code(), Some(2), "{with_command:?}");
    let envelope = json_of(stderr_of(&with_command).trim().as_bytes());
    assert_eq!(envelope["error"], "argument-conflict", "{envelope}");

    let binary = tmp.path().join("target/release/xr");
    let with_path = cmd()
        .args([
            "audit",
            binary.to_str().expect("utf-8"),
            "--bin",
            "xr",
            "--output",
            "json",
        ])
        .output()
        .expect("spawn anc");
    assert_eq!(with_path.status.code(), Some(2), "{with_path:?}");
    assert!(with_path.stdout.is_empty(), "{with_path:?}");
    let envelope = json_of(stderr_of(&with_path).trim().as_bytes());
    assert_eq!(envelope["kind"], "usage", "{envelope}");
    assert_eq!(envelope["error"], "bin-needs-directory", "{envelope}");
    assert_eq!(envelope["bin"], "xr", "{envelope}");
    assert_eq!(
        envelope["next_step"],
        serde_json::json!({
            "action": "rerun",
            "command": format!("anc audit {} --output json", binary.display()),
            "docs": BIN_SELECTION_DOCS,
        }),
        "{envelope}"
    );
}

#[test]
#[cfg(unix)]
fn candidates_sharing_a_name_are_told_apart_by_path() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(
        root,
        "cli/Cargo.toml",
        "[package]\nname = \"tool\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
    );
    write(root, "cli/src/main.rs", "fn main() {}\n");
    fixture_cli(&root.join("cli/target/release/tool"), "1.0.0-rust");
    write(
        root,
        "py/pyproject.toml",
        "[project]\nname = \"pytool\"\nversion = \"2.0.0\"\n\n[project.scripts]\ntool = \"pytool:main\"\n",
    );
    fixture_cli(&root.join("py/.venv/bin/tool"), "2.0.0-py");

    let ambiguous = cmd()
        .current_dir(root)
        .args(["audit", "."])
        .output()
        .expect("spawn anc");
    assert_eq!(ambiguous.status.code(), Some(2), "{ambiguous:?}");
    let stderr = stderr_of(&ambiguous);
    assert!(
        stderr.contains("anc audit . --bin cli/target/release/tool"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains("anc audit . --bin py/.venv/bin/tool"),
        "stderr: {stderr}"
    );

    let picked = cmd()
        .current_dir(root)
        .args([
            "audit",
            ".",
            "--bin",
            "py/.venv/bin/tool",
            "--output",
            "json",
        ])
        .output()
        .expect("spawn anc");
    let card = json_of(&picked.stdout);
    assert_eq!(card["tool"]["version"], "2.0.0", "{card}");
}

#[test]
fn no_built_binary_warns_with_the_declared_bins_and_the_ways_forward() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(
        root,
        "Cargo.toml",
        "[package]\nname = \"tool\"\nversion = \"1.0.0\"\nedition = \"2024\"\n",
    );
    write(root, "src/main.rs", "fn main() {}\n");

    let output = cmd()
        .current_dir(root)
        .args(["audit", ".", "--output", "json"])
        .output()
        .expect("spawn anc");

    let stderr = stderr_of(&output);
    let warning = stderr
        .lines()
        .find(|line| line.starts_with("warning:") && line.contains("tool"))
        .unwrap_or_else(|| panic!("no warning names the declared bin; stderr: {stderr}"));
    for way in ["build", "path", "--command"] {
        assert!(
            warning.contains(way),
            "the warning names `{way}`: {warning}"
        );
    }
    let card = json_of(&output.stdout);
    assert!(
        card["results"]
            .as_array()
            .expect("results")
            .iter()
            .any(|row| row["layer"] == "source"),
        "source audits still run: {card}"
    );
}

/// The scorecard row that `audit_id` produced, if any.
fn row<'a>(card: &'a serde_json::Value, audit_id: &str) -> Option<&'a serde_json::Value> {
    card["results"]
        .as_array()
        .expect("results")
        .iter()
        .find(|row| row["audit_id"] == audit_id)
}

fn audit_json(root: &Path, extra: &[&str]) -> serde_json::Value {
    let output = cmd()
        .current_dir(root)
        .args(["audit", ".", "--output", "json"])
        .args(extra)
        .output()
        .expect("spawn anc");
    json_of(&output.stdout)
}

const CLI_DEPENDENCIES: &str = "\n[dependencies]\nanyhow = \"1\"\nclap = \"4\"\nserde = \"1\"\n";

#[test]
#[cfg(unix)]
fn a_workspace_root_reads_the_graded_crates_manifest_and_version() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"3\"\n",
    );
    write(
        root,
        "crates/cli/Cargo.toml",
        &format!(
            "[package]\nname = \"cli\"\nversion = \"4.2.0\"\nedition = \"2024\"\n{CLI_DEPENDENCIES}"
        ),
    );
    write(root, "crates/cli/src/main.rs", "fn main() {}\n");
    fixture_cli(&root.join("target/release/cli"), "9.9.9");

    let card = audit_json(root, &[]);

    let dependencies = row(&card, "p6-dependencies").expect("p6-dependencies row");
    assert_eq!(dependencies["status"], "pass", "{dependencies}");
    assert_eq!(card["tool"]["version"], "4.2.0", "{card}");
}

#[test]
#[cfg(unix)]
fn a_mixed_repo_runs_rust_and_python_source_audits() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(
        root,
        "cli/Cargo.toml",
        "[package]\nname = \"tool\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(
        root,
        "cli/src/main.rs",
        "fn main() {\n    println!(\"tool\");\n}\n",
    );
    write(
        root,
        "py/pyproject.toml",
        "[project]\nname = \"pytool\"\nversion = \"0.1.0\"\n",
    );
    write(
        root,
        "py/src/pytool/__init__.py",
        "def main():\n    print('pytool')\n",
    );
    fixture_cli(&root.join("cli/target/release/tool"), "0.1.0");

    let card = audit_json(root, &[]);

    assert_eq!(card["tool"]["binary"], "tool", "{card}");
    assert!(
        row(&card, "code-unwrap").is_some(),
        "a Rust-only audit ran: {card}"
    );
    assert!(
        row(&card, "code-bare-except").is_some(),
        "a Python-only audit ran: {card}"
    );
}

#[test]
#[cfg(unix)]
fn an_audit_both_languages_run_yields_one_row_per_requirement() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(
        root,
        "cli/Cargo.toml",
        "[package]\nname = \"tool\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(root, "cli/src/main.rs", "fn main() {}\n");
    write(
        root,
        "py/pyproject.toml",
        "[project]\nname = \"pytool\"\nversion = \"0.1.0\"\n",
    );
    write(root, "py/src/pytool/__init__.py", "def main():\n    pass\n");
    fixture_cli(&root.join("cli/target/release/tool"), "0.1.0");

    let card = audit_json(root, &[]);

    let ids: Vec<&str> = card["results"]
        .as_array()
        .expect("results")
        .iter()
        .filter(|row| row["audit_id"] == "p6-sigterm")
        .filter_map(|row| row["id"].as_str())
        .collect();
    let mut unique = ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert!(!ids.is_empty(), "p6-sigterm ran: {card}");
    assert_eq!(ids.len(), unique.len(), "one row per requirement: {ids:?}");
}

#[test]
fn with_nothing_graded_one_declaring_package_supplies_the_manifest() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write(
        root,
        "lib/Cargo.toml",
        "[package]\nname = \"core\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(root, "lib/src/lib.rs", "");
    write(
        root,
        "cli/Cargo.toml",
        &format!(
            "[package]\nname = \"tool\"\nversion = \"0.1.0\"\nedition = \"2024\"\n{CLI_DEPENDENCIES}"
        ),
    );
    write(root, "cli/src/main.rs", "fn main() {}\n");

    let card = audit_json(root, &[]);

    let dependencies = row(&card, "p6-dependencies").expect("p6-dependencies row");
    assert_eq!(dependencies["status"], "pass", "{dependencies}");
}

#[test]
fn with_nothing_graded_two_declaring_packages_skip_the_manifest_audits() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    for name in ["alpha", "beta"] {
        write(
            root,
            &format!("{name}/Cargo.toml"),
            &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
        );
        write(root, &format!("{name}/src/main.rs"), "fn main() {}\n");
    }

    let card = audit_json(root, &[]);

    let dependencies = row(&card, "p6-dependencies").expect("p6-dependencies row");
    assert_eq!(dependencies["status"], "skip", "{dependencies}");
    let evidence = dependencies["evidence"].as_str().expect("evidence");
    assert!(
        evidence.contains("alpha") && evidence.contains("beta"),
        "{evidence}"
    );
}

#[test]
fn rust_source_the_repository_ignores_is_not_audited() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    fs::create_dir_all(root.join(".git")).expect("mkdir .git");
    write(root, ".gitignore", "generated/\n");
    write(
        root,
        "Cargo.toml",
        "[package]\nname = \"tool\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(root, "src/main.rs", "fn main() {}\n");
    write(
        root,
        "generated/bad.rs",
        "pub fn f(x: Option<u8>) -> u8 {\n    x.unwrap()\n}\n",
    );

    let card = audit_json(root, &[]);

    let unwrap = row(&card, "code-unwrap").expect("code-unwrap row");
    assert_eq!(unwrap["status"], "pass", "{unwrap}");
}

#[test]
#[cfg(unix)]
fn the_ambiguity_envelope_says_what_to_run_next() {
    let tmp = two_binary_workspace();

    let output = cmd()
        .current_dir(tmp.path())
        .args(["audit", ".", "--output", "json"])
        .output()
        .expect("spawn anc");

    let envelope = json_of(stderr_of(&output).trim().as_bytes());
    assert_eq!(
        envelope["next_step"],
        serde_json::json!({
            "action": "choose-bin",
            "template": "anc audit . --output json --bin <name>",
            "docs": BIN_SELECTION_DOCS,
        }),
        "{envelope}"
    );
}

#[test]
#[cfg(unix)]
fn an_unknown_bin_envelope_echoes_the_name_and_keeps_the_callers_flags() {
    let tmp = two_binary_workspace();

    let output = cmd()
        .current_dir(tmp.path())
        .args([
            "audit",
            ".",
            "--principle",
            "6",
            "--bin=nope",
            "--output",
            "json",
        ])
        .output()
        .expect("spawn anc");

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let envelope = json_of(stderr_of(&output).trim().as_bytes());
    assert_eq!(envelope["error"], "unknown-bin", "{envelope}");
    assert_eq!(envelope["bin"], "nope", "{envelope}");
    assert_eq!(envelope["next_step"]["action"], "choose-bin", "{envelope}");
    let commands: Vec<&str> = envelope["candidates"]
        .as_array()
        .expect("candidates array")
        .iter()
        .filter_map(|c| c["command"].as_str())
        .collect();
    for expected in [
        "anc audit . --principle 6 --output json --bin xr",
        "anc audit . --principle 6 --output json --bin xdk-consumer-check",
    ] {
        assert!(commands.contains(&expected), "{expected}: {envelope}");
    }
}

#[test]
#[cfg(unix)]
fn text_mode_commands_keep_the_callers_flags() {
    let tmp = two_binary_workspace();

    let output = cmd()
        .current_dir(tmp.path())
        .args(["audit", ".", "-q", "--principle", "6"])
        .output()
        .expect("spawn anc");

    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("anc audit . -q --principle 6 --bin xr "),
        "stderr: {stderr}"
    );
}

#[test]
#[cfg(unix)]
fn source_only_needs_no_bin_where_several_binaries_are_built() {
    let tmp = two_binary_workspace();

    let output = cmd()
        .current_dir(tmp.path())
        .args(["audit", ".", "--source", "--output", "json"])
        .output()
        .expect("spawn anc");

    let stderr = stderr_of(&output);
    assert!(!stderr.contains("binary-ambiguous"), "stderr: {stderr}");
    let card = json_of(&output.stdout);
    let rows = card["results"].as_array().expect("results");
    assert!(!rows.is_empty(), "source audits ran: {card}");
    assert!(
        rows.iter().all(|row| row["layer"] != "behavioral"),
        "no behavioral audits: {card}"
    );
}

#[test]
#[cfg(unix)]
fn source_only_still_rejects_a_bin_that_names_nothing() {
    let tmp = two_binary_workspace();

    let output = cmd()
        .current_dir(tmp.path())
        .args([
            "audit", ".", "--source", "--bin", "nope", "--output", "json",
        ])
        .output()
        .expect("spawn anc");

    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let envelope = json_of(stderr_of(&output).trim().as_bytes());
    assert_eq!(envelope["error"], "unknown-bin", "{envelope}");
}

/// A crate whose only `.unwrap()` sits in `rel`.
fn crate_with_unwrap_in(root: &Path, rel: &str) {
    write(
        root,
        "Cargo.toml",
        "[package]\nname = \"tool\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(root, "src/main.rs", "fn main() {}\n");
    write(
        root,
        rel,
        "pub fn f(x: Option<u8>) -> u8 {\n    x.unwrap()\n}\n",
    );
}

#[test]
fn example_programs_beside_a_manifest_are_not_audited() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    crate_with_unwrap_in(root, "crates/lib/examples/demo.rs");
    write(
        root,
        "crates/lib/Cargo.toml",
        "[package]\nname = \"lib\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    write(root, "crates/lib/src/lib.rs", "");
    write(
        root,
        "examples/top.rs",
        "fn main() { Some(1u8).unwrap(); }\n",
    );

    let card = audit_json(root, &["--include-tests"]);

    let unwrap = row(&card, "code-unwrap").expect("code-unwrap row");
    assert_eq!(unwrap["status"], "pass", "{unwrap}");
}

#[test]
fn a_source_module_named_examples_is_still_audited() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    crate_with_unwrap_in(root, "src/commands/examples/mod.rs");

    let card = audit_json(root, &[]);

    let unwrap = row(&card, "code-unwrap").expect("code-unwrap row");
    assert_ne!(unwrap["status"], "pass", "{unwrap}");
    assert!(
        unwrap["evidence"]
            .as_str()
            .is_some_and(|evidence| evidence.contains("src/commands/examples/mod.rs")),
        "{unwrap}"
    );
}
