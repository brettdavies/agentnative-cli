//! End-to-end coverage for directory-target discovery: the package
//! inventory, binary selection, and what each run audits.

use std::fs;
use std::path::Path;

use assert_cmd::Command;

/// A user-level config path nothing creates, so no audit reads the
/// developer's own `~/.anc.toml`.
const NO_HOME_CONFIG: &str = concat!(env!("CARGO_TARGET_TMPDIR"), "/no-home/.anc.toml");

fn cmd() -> Command {
    let mut cmd = Command::cargo_bin("anc").expect("anc binary should exist");
    cmd.env("AGENTNATIVE_HOME_CONFIG", NO_HOME_CONFIG);
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
