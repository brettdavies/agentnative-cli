//! End-to-end coverage for the `p6-may-standard-names` audit's `.anc.toml`
//! integration. Exercises the loader + audit through the real `anc` binary
//! so the parse-error -> Warn path and the domain-verb -> Pass path are
//! verified against the published surface.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use assert_cmd::Command;

/// Build a Command for the anc binary.
fn cmd() -> Command {
    Command::cargo_bin("anc").expect("binary should exist")
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
const FIXTURE_SCRIPT: &str = r#"#!/bin/sh
case "$1" in
  --help) cat <<'EOF'
Usage: x [OPTIONS] <COMMAND>

Commands:
  archive    Archive a post
  follow     Follow a user
  mentions   List mentions

Options:
  -h, --help     Show help
  -V, --version  Print version
EOF
    exit 0 ;;
  --version) echo "x 0.1.0"; exit 0 ;;
  *) echo "x tool"; exit 0 ;;
esac
"#;

/// Stage `dir` as a Python project (`pyproject.toml`) whose `dist/`
/// directory holds the fixture binary. `Project::discover` resolves
/// directory targets via manifest detection; a Python manifest is the
/// cheapest way to opt into `discover_simple_binaries`, which picks up
/// every executable under `dist/` as a runner candidate. The directory
/// path is the audit target, which is what makes `.anc.toml` discovery
/// fire — passing the binary file directly would bypass the loader by
/// design.
fn stage_project(dir: &std::path::Path) -> PathBuf {
    fs::write(
        dir.join("pyproject.toml"),
        "[project]\nname = \"x\"\nversion = \"0.1.0\"\n",
    )
    .expect("write pyproject.toml");
    let dist = dir.join("dist");
    fs::create_dir_all(&dist).expect("mkdir dist");
    let bin = dist.join("x");
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o755)
            .open(&bin)
            .expect("open fixture binary");
        f.write_all(FIXTURE_SCRIPT.as_bytes())
            .expect("write fixture binary");
    }
    #[cfg(not(unix))]
    {
        fs::write(&bin, FIXTURE_SCRIPT).expect("write fixture binary");
    }
    bin
}

/// Run `anc audit <bin> --output json` and pluck the audit row for
/// `p6-may-standard-names`. Returns `(status, evidence)` where evidence is
/// the empty string when absent (Pass rows carry no evidence).
fn run_audit_and_extract(target: &std::path::Path) -> (String, String) {
    let assert = cmd()
        .args([
            "audit",
            target.to_str().expect("utf8 path"),
            "--output",
            "json",
        ])
        .assert();

    let output = assert.get_output().stdout.clone();
    let json_str = String::from_utf8(output).expect("stdout valid UTF-8");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("scorecard is valid JSON");

    let results = parsed["results"]
        .as_array()
        .expect("scorecard.results is an array");

    let row = results
        .iter()
        .find(|r| r["id"].as_str() == Some("p6-may-standard-names"))
        .expect("scorecard contains p6-may-standard-names row");

    let status = row["status"]
        .as_str()
        .expect("status is a string")
        .to_string();
    let evidence = row["evidence"].as_str().unwrap_or("").to_string();
    (status, evidence)
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

    // Run the audit against the directory so `.anc.toml` is discoverable
    // (binary-mode targets sidestep the loader by design).
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
