//! Integration tests for `anc skill install`. Spawns the real binary;
//! introspects stdout / stderr / exit code. Tests numbered 13-23 per the
//! plan's `Test scenarios` section. (Tests 1-12 live in `src/skill_install.rs`
//! as unit tests; 24-25 live in `tests/dogfood.rs`; test 26 is a CI step.)

use assert_cmd::Command;
use serde_json::Value;

fn cmd() -> Command {
    Command::cargo_bin("anc").expect("anc binary should exist")
}

/// Build a clean `anc` invocation for tests that should be insensitive to
/// the host's `$HOME` and `$PATH`. Each caller can re-`.env(…)` as needed.
fn cmd_with_home(home: &std::path::Path) -> Command {
    let mut c = cmd();
    c.env("HOME", home);
    c
}

// -------------------------------------------------------------------------
// Test 13 — dry-run claude_code text mode prints a single-line `git clone`
// command on stdout and exits 0.
// -------------------------------------------------------------------------
#[test]
fn dry_run_claude_code_text_prints_single_line_command() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = cmd_with_home(tmp.path())
        .args(["skill", "install", "--dry-run", "claude_code"])
        .output()
        .expect("anc spawn");

    assert!(
        out.status.success(),
        "expected exit 0; got {:?}",
        out.status
    );
    let stdout = String::from_utf8(out.stdout).expect("utf-8 stdout");
    let trimmed = stdout.trim_end_matches('\n');
    assert!(
        !trimmed.contains('\n'),
        "dry-run text mode must be single-line; got {trimmed:?}",
    );
    assert!(
        trimmed.starts_with("git clone --depth 1 "),
        "expected leading `git clone --depth 1 `; got {trimmed:?}",
    );
    assert!(
        trimmed.contains(".claude/skills/agent-native-cli"),
        "expected the canonical claude_code dest path; got {trimmed:?}",
    );
}

// -------------------------------------------------------------------------
// Test 14 — dry-run claude_code json mode produces the envelope schema with
// mode=dry-run, would_succeed=true, status=success.
// -------------------------------------------------------------------------
#[test]
fn dry_run_claude_code_json_emits_success_envelope() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = cmd_with_home(tmp.path())
        .args([
            "skill",
            "install",
            "--dry-run",
            "claude_code",
            "--output",
            "json",
        ])
        .output()
        .expect("anc spawn");

    assert!(out.status.success(), "expected exit 0");
    let stdout = String::from_utf8(out.stdout).expect("utf-8 stdout");
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON envelope");

    assert_eq!(v["action"], "skill-install");
    assert_eq!(v["host"], "claude_code");
    assert_eq!(v["mode"], "dry-run");
    assert_eq!(v["status"], "success");
    assert_eq!(v["would_succeed"], true);
    assert!(v["exit_code"].is_null(), "dry-run mode omits exit_code");
    assert!(v["reason"].is_null(), "success path omits reason");
    assert_eq!(v["destination_status"], "absent");
    assert!(
        v["command"]
            .as_str()
            .unwrap()
            .starts_with("git clone --depth 1 "),
        "command must start with the canonical clone prefix",
    );
    assert!(
        v["destination"]
            .as_str()
            .unwrap()
            .contains(".claude/skills/agent-native-cli"),
        "destination must resolve to the claude_code canonical path",
    );
}

// -------------------------------------------------------------------------
// Test 15 — dry-run + pre-placed regular file at the canonical dest yields
// status=error, reason=destination-is-file, would_succeed=false, exit 1.
// -------------------------------------------------------------------------
#[test]
fn dry_run_with_regular_file_at_dest_emits_destination_is_file() {
    let tmp = tempfile::tempdir().expect("tempdir");
    // Pre-place a regular file at the resolved canonical dest:
    //   $HOME/.claude/skills/agent-native-cli
    let dest_dir = tmp.path().join(".claude/skills");
    std::fs::create_dir_all(&dest_dir).expect("mkdir parent");
    std::fs::write(dest_dir.join("agent-native-cli"), b"not-a-dir").expect("write file");

    let out = cmd_with_home(tmp.path())
        .args([
            "skill",
            "install",
            "--dry-run",
            "claude_code",
            "--output",
            "json",
        ])
        .output()
        .expect("anc spawn");

    assert_eq!(out.status.code(), Some(1), "expected exit 1");
    let stdout = String::from_utf8(out.stdout).expect("utf-8 stdout");
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON envelope");

    assert_eq!(v["status"], "error");
    assert_eq!(v["reason"], "destination-is-file");
    assert_eq!(v["would_succeed"], false);
    assert_eq!(v["destination_status"], "file");
    assert!(
        v["exit_code"].is_null(),
        "no spawn happened — exit_code must be absent"
    );
}

// -------------------------------------------------------------------------
// Test 16b — `#[ignore]` end-to-end. Spawns the real binary with HOME set
// to a tempdir; runs the actual git clone against the upstream skill repo.
// Excluded from default `cargo test` since it depends on network and the
// public GitHub repo. Run with `cargo test -- --ignored skill_install` when
// vetting a release.
// -------------------------------------------------------------------------
#[test]
#[ignore = "network: clones from github.com/brettdavies/agentnative-skill"]
fn live_install_clones_into_canonical_dest() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let out = cmd_with_home(tmp.path())
        .args(["skill", "install", "claude_code", "--output", "json"])
        .output()
        .expect("anc spawn");

    let stdout = String::from_utf8(out.stdout).expect("utf-8 stdout");
    assert!(
        out.status.success(),
        "live install failed; stdout={stdout}, stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );
    let dest = tmp.path().join(".claude/skills/agent-native-cli");
    assert!(
        dest.join(".git").is_dir(),
        "expected .git/ at {}",
        dest.display()
    );
}

// -------------------------------------------------------------------------
// Test 17 — clap rejects unknown host with exit 2 and lists possible values.
// -------------------------------------------------------------------------
#[test]
fn unknown_host_rejected_with_clap_exit_2() {
    let out = cmd()
        .args(["skill", "install", "definitely-not-a-host"])
        .output()
        .expect("anc spawn");

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).expect("utf-8 stderr");
    assert!(
        stderr.contains("invalid value"),
        "expected clap 'invalid value' message; got {stderr:?}",
    );
    for host in ["claude_code", "codex", "cursor", "opencode"] {
        assert!(
            stderr.contains(host),
            "expected possible value {host:?} in error; got {stderr:?}",
        );
    }
}

// -------------------------------------------------------------------------
// Test 18 — clap rejects missing positional host with exit 2.
// -------------------------------------------------------------------------
#[test]
fn missing_host_rejected_with_clap_exit_2() {
    let out = cmd()
        .args(["skill", "install"])
        .output()
        .expect("anc spawn");

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8(out.stderr).expect("utf-8 stderr");
    assert!(
        stderr.contains("required") && stderr.contains("HOST"),
        "expected required-arg-<HOST> message; got {stderr:?}",
    );
}

// -------------------------------------------------------------------------
// Test 19 — live install on already-populated destination → envelope with
// status=error, reason=destination-not-empty, exit_code absent (we never
// spawned).
// -------------------------------------------------------------------------
#[test]
fn live_install_on_populated_dest_does_not_spawn() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dest = tmp.path().join(".claude/skills/agent-native-cli");
    std::fs::create_dir_all(&dest).expect("mkdir dest");
    std::fs::write(dest.join("placeholder"), b"x").expect("populate dest");

    let out = cmd_with_home(tmp.path())
        .args(["skill", "install", "claude_code", "--output", "json"])
        .output()
        .expect("anc spawn");

    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8 stdout");
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON envelope");

    assert_eq!(v["status"], "error");
    assert_eq!(v["reason"], "destination-not-empty");
    assert_eq!(v["mode"], "install");
    assert_eq!(v["destination_status"], "non-empty-dir");
    assert!(
        v["exit_code"].is_null(),
        "destination check failed — never spawned git, exit_code must be null",
    );
    assert!(
        v["would_succeed"].is_null(),
        "would_succeed is dry-run-only"
    );
}

// -------------------------------------------------------------------------
// Test 20 — HOME unset surfaces reason=home-not-set, exit 1.
// -------------------------------------------------------------------------
#[test]
fn home_unset_emits_home_not_set_envelope() {
    let out = cmd()
        .env_remove("HOME")
        .args(["skill", "install", "claude_code", "--output", "json"])
        .output()
        .expect("anc spawn");

    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8 stdout");
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON envelope");

    assert_eq!(v["status"], "error");
    assert_eq!(v["reason"], "home-not-set");
    assert!(
        v["destination"].as_str().unwrap().starts_with("~"),
        "MissingHome surfaces the unexpanded template in destination",
    );
    assert_eq!(v["destination_status"], "absent");
}

// -------------------------------------------------------------------------
// Test 21 — `git` not on PATH surfaces reason=git-not-found, exit 1.
// Pinning PATH to an empty directory makes `git` unresolvable on Unix
// (where Command::status reports NotFound). Skipped on Windows since the
// test infra would need a different shape there.
// -------------------------------------------------------------------------
#[cfg(unix)]
#[test]
fn git_not_on_path_emits_git_not_found_envelope() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let empty_path = tmp.path().to_str().expect("utf-8 tempdir");

    let out = cmd_with_home(tmp.path())
        .env("PATH", empty_path)
        .args(["skill", "install", "claude_code", "--output", "json"])
        .output()
        .expect("anc spawn");

    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("utf-8 stdout");
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON envelope");

    assert_eq!(v["status"], "error");
    assert_eq!(v["reason"], "git-not-found");
    assert!(
        v["exit_code"].is_null(),
        "git not on PATH means we never spawned — exit_code must be null",
    );
}

// -------------------------------------------------------------------------
// Test 22 — `arg_required_else_help_unaffected_by_skill_subcommand`: bare
// `anc skill` prints help and exits with code 2. Pins the fork-bomb-safety
// invariant from CLAUDE.md ("Bare invocation prints help"). Catches the
// regression where adding the skill subcommand accidentally drops
// arg_required_else_help on the parent.
// -------------------------------------------------------------------------
#[test]
fn arg_required_else_help_unaffected_by_skill_subcommand() {
    let out = cmd().arg("skill").output().expect("anc spawn");
    assert_eq!(
        out.status.code(),
        Some(2),
        "bare `anc skill` must exit with code 2 (clap missing-subcommand)",
    );
    let stderr = String::from_utf8(out.stderr).unwrap_or_default();
    let stdout = String::from_utf8(out.stdout).unwrap_or_default();
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("Usage:"),
        "expected help text mentioning Usage; got stdout={stdout:?} stderr={stderr:?}",
    );
}

// -------------------------------------------------------------------------
// Test 23 — `exit_codes_match_p4_convention`: pins the P4 exit-code shape
// in one place rather than scattering assertions across tests 15/19/20/21.
// Table-driven across the user-error envelope cases. happy=0 is asserted in
// test 13/14; internal-error=2 is reserved (clap surfaces it; tests 17/18
// pin that side).
// -------------------------------------------------------------------------
#[test]
fn exit_codes_match_p4_convention() {
    struct Case {
        name: &'static str,
        setup: Box<dyn FnOnce(&std::path::Path) -> Command>,
        expected_exit: i32,
        expected_reason: &'static str,
    }

    let cases: Vec<Case> = vec![
        Case {
            name: "DestNotEmpty",
            setup: Box::new(|home: &std::path::Path| {
                let dest = home.join(".claude/skills/agent-native-cli");
                std::fs::create_dir_all(&dest).expect("mkdir");
                std::fs::write(dest.join("x"), b"y").expect("populate");
                let mut c = cmd_with_home(home);
                c.args(["skill", "install", "claude_code", "--output", "json"]);
                c
            }),
            expected_exit: 1,
            expected_reason: "destination-not-empty",
        },
        Case {
            name: "DestIsFile",
            setup: Box::new(|home: &std::path::Path| {
                let parent = home.join(".claude/skills");
                std::fs::create_dir_all(&parent).expect("mkdir parent");
                std::fs::write(parent.join("agent-native-cli"), b"not-a-dir").expect("write file");
                let mut c = cmd_with_home(home);
                c.args(["skill", "install", "claude_code", "--output", "json"]);
                c
            }),
            expected_exit: 1,
            expected_reason: "destination-is-file",
        },
        Case {
            name: "MissingHome",
            setup: Box::new(|_home: &std::path::Path| {
                let mut c = cmd();
                c.env_remove("HOME");
                c.args(["skill", "install", "claude_code", "--output", "json"]);
                c
            }),
            expected_exit: 1,
            expected_reason: "home-not-set",
        },
        #[cfg(unix)]
        Case {
            name: "GitNotFound",
            setup: Box::new(|home: &std::path::Path| {
                let mut c = cmd_with_home(home);
                c.env("PATH", home.to_str().expect("utf-8"));
                c.args(["skill", "install", "claude_code", "--output", "json"]);
                c
            }),
            expected_exit: 1,
            expected_reason: "git-not-found",
        },
    ];

    for case in cases {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut command = (case.setup)(tmp.path());
        let out = command
            .output()
            .unwrap_or_else(|e| panic!("anc spawn for {}: {e}", case.name));
        assert_eq!(
            out.status.code(),
            Some(case.expected_exit),
            "{}: expected exit {}, got {:?}",
            case.name,
            case.expected_exit,
            out.status.code(),
        );
        let stdout = String::from_utf8(out.stdout).expect("utf-8 stdout");
        let v: Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
            panic!(
                "{}: expected JSON envelope; parse failed: {e}; stdout={stdout}",
                case.name
            )
        });
        assert_eq!(
            v["reason"], case.expected_reason,
            "{}: reason mismatch",
            case.name,
        );
    }
}
