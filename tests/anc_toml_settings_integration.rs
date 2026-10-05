//! End-to-end coverage for the `.anc.toml` settings audits read beyond
//! `[p6] domain_verbs`: each test runs the real `anc` binary against a shell
//! fixture and reads one scorecard row, so the loader, the audit, and the
//! row's evidence are checked together.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

/// A fixture CLI in its own directory, with a separate directory for the
/// repository `.anc.toml` (`--repo`) and one for the user-level file.
struct Fixture {
    _tmp: tempfile::TempDir,
    bin: PathBuf,
    repo: PathBuf,
    home_file: PathBuf,
}

impl Fixture {
    /// `script` is the body of a `case "$*" in ... esac` dispatch on the
    /// fixture's arguments.
    fn new(script: &str) -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = tmp.path().to_path_buf();
        let bin = base.join("bin").join("tool");
        let repo = base.join("repo");
        let home = base.join("home");
        for dir in [bin.parent().expect("bin dir"), &repo, &home] {
            fs::create_dir_all(dir).expect("create fixture dir");
        }
        write_executable(
            &bin,
            &format!("#!/bin/sh\ncase \"$*\" in\n{script}\nesac\n"),
        );
        let home_file = home.join(".anc.toml");
        fs::write(&home_file, "").expect("write empty home config");
        Self {
            _tmp: tmp,
            bin,
            repo,
            home_file,
        }
    }

    fn repo_config(&self, body: &str) -> &Self {
        fs::write(self.repo.join(".anc.toml"), body).expect("write repo .anc.toml");
        self
    }

    fn home_config(&self, body: &str) -> &Self {
        fs::write(&self.home_file, body).expect("write home .anc.toml");
        self
    }

    /// The `id` row of `anc audit <bin> --repo <repo> --output json`.
    fn row(&self, id: &str) -> Value {
        let output = Command::cargo_bin("anc")
            .expect("anc binary")
            .env("AGENTNATIVE_HOME_CONFIG", &self.home_file)
            .args(["audit", path_str(&self.bin), "--repo", path_str(&self.repo)])
            .args(["--output", "json"])
            .output()
            .expect("spawn anc");
        let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
        let scorecard: Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("scorecard is JSON ({e}): {stdout}"));
        scorecard["results"]
            .as_array()
            .expect("results array")
            .iter()
            .find(|row| row["id"] == id)
            .unwrap_or_else(|| panic!("no {id} row: {stdout}"))
            .clone()
    }
}

fn write_executable(path: &Path, body: &str) {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o755)
            .open(path)
            .expect("open fixture");
        file.write_all(body.as_bytes()).expect("write fixture");
    }
    #[cfg(not(unix))]
    fs::write(path, body).expect("write fixture");
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("utf8 path")
}

/// A terraform-shaped CLI: `destroy` confirms by prompt, and its help lists
/// the Go-style `-auto-approve` bypass.
const DESTROY_CLI: &str = r#"  "destroy --help") printf 'Usage: tool destroy [options]\n\nOptions:\n\n  -auto-approve  Skip interactive approval.\n' ;;
  "--version") echo "tool 1.0.0" ;;
  *) printf 'Usage: tool <COMMAND>\n\nCommands:\n  destroy  Destroy everything\n  list     List things\n' ;;"#;

#[test]
fn confirm_flags_from_the_repo_file_pass_force_yes_and_name_the_file() {
    let fixture = Fixture::new(DESTROY_CLI);
    let without = fixture.row("p5-must-force-yes");
    assert_eq!(without["status"], "fail", "row: {without}");

    let row = fixture
        .repo_config("[p5]\nconfirm_flags = [\"-auto-approve\"]\n")
        .row("p5-must-force-yes");

    assert_eq!(row["status"], "pass", "row: {row}");
    assert_eq!(
        row["evidence"], "destroy accepts -auto-approve via .anc.toml [p5].confirm_flags",
        "row: {row}"
    );
}

#[test]
fn confirm_flags_in_both_files_credit_the_repo_file() {
    let fixture = Fixture::new(DESTROY_CLI);
    fixture.home_config("[p5]\nconfirm_flags = [\"-auto-approve\"]\n");
    let home_only = fixture.row("p5-must-force-yes");
    assert_eq!(
        home_only["evidence"],
        "destroy accepts -auto-approve via $AGENTNATIVE_HOME_CONFIG [p5].confirm_flags",
        "row: {home_only}"
    );

    let row = fixture
        .repo_config("[p5]\nconfirm_flags = [\"-auto-approve\"]\n")
        .row("p5-must-force-yes");

    assert_eq!(
        row["evidence"], "destroy accepts -auto-approve via .anc.toml [p5].confirm_flags",
        "row: {row}"
    );
}

/// A biome-shaped CLI: `clean` removes log files and takes no confirmation
/// flag; `delete` confirms with `--force`.
const CLEAN_CLI: &str = r#"  "clean --help") printf 'Usage: tool clean\n\nOptions:\n  -h, --help  Show help.\n' ;;
  "delete --help") printf 'Usage: tool delete <ID>\n\nOptions:\n      --force  Skip the prompt.\n' ;;
  "--version") echo "tool 1.0.0" ;;
  *) printf 'Usage: tool <COMMAND>\n\nCommands:\n  clean   Remove the log files\n  delete  Delete an item\n  list    List items\n' ;;"#;

#[test]
fn not_destructive_from_the_home_file_passes_force_yes_and_names_the_file() {
    let fixture = Fixture::new(CLEAN_CLI);
    let without = fixture.row("p5-must-force-yes");
    assert_eq!(without["status"], "fail", "row: {without}");

    let row = fixture
        .home_config("[p5]\nnot_destructive = [\"clean\"]\n")
        .row("p5-must-force-yes");

    assert_eq!(row["status"], "pass", "row: {row}");
    assert_eq!(
        row["evidence"],
        "declared not destructive: clean via $AGENTNATIVE_HOME_CONFIG [p5].not_destructive",
        "row: {row}"
    );
}

/// A kubectl-shaped CLI: the top-level help names no output format, `get`
/// carries `-o, --output`, the safe probes answer in text, and
/// `version --client -o json` prints JSON.
const KUBECTL_LIKE_CLI: &str = r#"  "version --client -o json") echo '{"clientVersion":{"gitVersion":"v1.0.0"}}' ;;
  "get --help") printf 'Display one or many resources.\n\nOptions:\n  -o, --output='"''"': One of: json, yaml, wide.\n' ;;
  "--version") echo "tool 1.0.0" ;;
  *) printf 'tool controls the thing.\n\nBasic Commands:\n  get      Display one or many resources\n  version  Print the version\n' ;;"#;

#[test]
fn json_probe_from_the_repo_file_validates_json_output_and_names_the_probe() {
    let fixture = Fixture::new(KUBECTL_LIKE_CLI);
    let without = fixture.row("p2-must-output-flag");
    assert_eq!(without["status"], "skip", "row: {without}");

    let row = fixture
        .repo_config("[p2]\njson_probe = [\"version\", \"--client\", \"-o\", \"json\"]\n")
        .row("p2-must-output-flag");

    assert_eq!(row["status"], "pass", "row: {row}");
    assert_eq!(
        row["evidence"],
        "`tool version --client -o json` printed JSON; probe declared via .anc.toml [p2].json_probe",
        "row: {row}"
    );
}
