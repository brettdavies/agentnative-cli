//! Pins the git-hardening surface of `scripts/sync-web-audit.sh`: every git
//! call runs with user and system config disabled, credential prompts off,
//! the SSH, proxy, askpass and exec-path overrides stripped, and the five
//! `-c` flags `src/skill_install.rs` applies to `anc skill install`. A `git`
//! shim on `PATH` records what the first invocation received and then fails,
//! so the test needs no network and no site checkout.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

const EXPECTED_FLAGS: [&str; 10] = [
    "-c",
    "credential.helper=",
    "-c",
    "core.askPass=",
    "-c",
    "protocol.allow=never",
    "-c",
    "protocol.https.allow=always",
    "-c",
    "http.followRedirects=false",
];

/// Every environment variable the script must strip before calling git.
const STRIPPED: [&str; 5] = [
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_PROXY_COMMAND",
    "GIT_ASKPASS",
    "GIT_EXEC_PATH",
];

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn every_git_call_carries_the_hardening_flags_and_environment() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let shim_dir = tmp.path().join("bin");
    fs::create_dir(&shim_dir).unwrap();
    let log = tmp.path().join("git.log");
    let shim = shim_dir.join("git");
    fs::write(
        &shim,
        format!(
            "#!/usr/bin/env bash\n\
             {{\n\
               printf 'ARGV'; for a in \"$@\"; do printf ' [%s]' \"$a\"; done; printf '\\n'\n\
               printf 'GIT_CONFIG_GLOBAL=%s\\n' \"${{GIT_CONFIG_GLOBAL-<unset>}}\"\n\
               printf 'GIT_CONFIG_SYSTEM=%s\\n' \"${{GIT_CONFIG_SYSTEM-<unset>}}\"\n\
               printf 'GIT_TERMINAL_PROMPT=%s\\n' \"${{GIT_TERMINAL_PROMPT-<unset>}}\"\n\
               for v in GIT_SSH GIT_SSH_COMMAND GIT_PROXY_COMMAND GIT_ASKPASS GIT_EXEC_PATH; do\n\
                 printf '%s=%s\\n' \"$v\" \"${{!v-<unset>}}\"\n\
               done\n\
             }} >> {log}\n\
             exit 1\n",
            log = log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();

    let path = format!(
        "{}:{}",
        shim_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let dest = tmp.path().join("dest");
    fs::create_dir(&dest).unwrap();
    let mut cmd = Command::new("bash");
    cmd.arg(repo_root().join("scripts/sync-web-audit.sh"))
        .arg("--check")
        .env("PATH", path)
        .env("WEB_AUDIT_SITE_SHA", "0".repeat(40))
        .env("WEB_AUDIT_DEST_ROOT", &dest)
        .env(
            "WEB_AUDIT_SITE_REMOTE_URL",
            "https://example.invalid/site.git",
        );
    for var in STRIPPED {
        cmd.env(var, "leaked");
    }
    let output = cmd.output().expect("run the sync script");
    assert!(
        !output.status.success(),
        "the shim fails every git call, so the script must fail"
    );

    let recorded = fs::read_to_string(&log).expect("the shim recorded the first git call");
    let argv_line = recorded
        .lines()
        .find(|l| l.starts_with("ARGV"))
        .expect("argv line");
    let expected: String = EXPECTED_FLAGS.iter().map(|f| format!(" [{f}]")).collect();
    assert!(
        argv_line.starts_with(&format!("ARGV{expected}")),
        "the five -c flags lead every git invocation: {argv_line}"
    );
    for line in [
        "GIT_CONFIG_GLOBAL=/dev/null",
        "GIT_CONFIG_SYSTEM=/dev/null",
        "GIT_TERMINAL_PROMPT=0",
    ] {
        assert!(recorded.contains(line), "{line} missing from {recorded}");
    }
    for var in STRIPPED {
        assert!(
            recorded.contains(&format!("{var}=<unset>")),
            "{var} must be stripped: {recorded}"
        );
    }
}

#[test]
fn an_unresolvable_pin_fails_naming_the_sha() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let sha = "f".repeat(40);
    let output = Command::new("bash")
        .arg(repo_root().join("scripts/sync-web-audit.sh"))
        .arg("--check")
        .env("WEB_AUDIT_SITE_SHA", &sha)
        .env("WEB_AUDIT_DEST_ROOT", tmp.path())
        .env("WEB_AUDIT_SITE_REPO", repo_root())
        .output()
        .expect("run the sync script");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&sha), "{stderr}");
    assert!(stderr.contains("is not in"), "{stderr}");
}

#[test]
fn a_short_pin_is_rejected_before_any_git_call() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let output = Command::new("bash")
        .arg(repo_root().join("scripts/sync-web-audit.sh"))
        .arg("--pin")
        .arg("abc123")
        .env("WEB_AUDIT_DEST_ROOT", tmp.path())
        .output()
        .expect("run the sync script");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("full 40-hex commit SHA"), "{stderr}");
}

#[test]
fn list_names_every_vendored_path_with_the_pin_last() {
    let output = Command::new("bash")
        .arg(repo_root().join("scripts/sync-web-audit.sh"))
        .arg("--list")
        .output()
        .expect("run the sync script");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<&str> = stdout.lines().collect();
    assert_eq!(paths.last(), Some(&"src/web_audit/vendored/SITE_SHA"));
    for path in &paths {
        assert!(
            repo_root().join(path).exists(),
            "{path} is not in this repository"
        );
    }
    assert!(paths.contains(&"src/web_audit/vendored/contract"));
    assert!(paths.contains(&"tests/fixtures/web-audit-contract"));
}

/// A throwaway site whose contract the sync script reads, and a destination synced
/// to it.
fn synced_copy(tmp: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf, String) {
    let git = |dir: &std::path::Path, args: &[&str]| -> String {
        let mut cmd = Command::new("git");
        cmd.current_dir(dir)
            .args([
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                // A developer's global ignore file commonly lists `build/`, which the
                // site's contract uses as a directory name.
                "-c",
                "core.excludesFile=/dev/null",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null");
        for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX"] {
            cmd.env_remove(var);
        }
        let output = cmd.output().expect("git runs");
        assert!(output.status.success(), "git {args:?}: {output:?}");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };
    let site = tmp.join("site");
    for file in [
        "src/data/web-audit/contract/build/registry.json",
        "src/data/web-audit/contract/test/score-parity.json",
    ] {
        let path = site.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{}\n").unwrap();
    }
    git(&site, &["init", "--quiet", "--initial-branch=dev"]);
    git(&site, &["add", "-A"]);
    git(&site, &["commit", "--quiet", "-m", "the contract"]);
    let sha = git(&site, &["rev-parse", "HEAD"]);
    let dest = tmp.join("dest");
    fs::create_dir(&dest).unwrap();
    let output = sync(&site, &dest).arg("--pin").arg(&sha).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    (site, dest, sha)
}

fn sync(site: &std::path::Path, dest: &std::path::Path) -> Command {
    let mut cmd = Command::new("bash");
    cmd.arg(repo_root().join("scripts/sync-web-audit.sh"))
        .env("WEB_AUDIT_SITE_REPO", site)
        .env("WEB_AUDIT_DEST_ROOT", dest);
    cmd
}

#[test]
fn check_passes_a_copy_that_matches_the_pin() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (site, dest, _) = synced_copy(tmp.path());
    let output = sync(&site, &dest).arg("--check").output().unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn check_names_a_file_only_the_copy_holds() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (site, dest, _) = synced_copy(tmp.path());
    fs::write(
        dest.join("src/web_audit/vendored/contract/extra.json"),
        "{}\n",
    )
    .unwrap();
    let output = sync(&site, &dest).arg("--check").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("extra.json"), "{stderr}");
}

#[test]
fn check_names_a_file_only_the_site_holds() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (site, dest, _) = synced_copy(tmp.path());
    fs::remove_file(dest.join("tests/fixtures/web-audit-contract/score-parity.json")).unwrap();
    let output = sync(&site, &dest).arg("--check").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("score-parity.json"), "{stderr}");
}

#[test]
fn check_names_a_file_whose_bytes_differ() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (site, dest, _) = synced_copy(tmp.path());
    fs::write(
        dest.join("src/web_audit/vendored/contract/registry.json"),
        "{\"edited\": true}\n",
    )
    .unwrap();
    let output = sync(&site, &dest).arg("--check").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("registry.json"), "{stderr}");
    assert!(!stderr.contains("score-parity.json"), "{stderr}");
}

#[test]
fn check_finds_drift_under_a_path_holding_a_sed_delimiter() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let nested = tmp.path().join("a|b");
    fs::create_dir(&nested).unwrap();
    let (site, dest, _) = synced_copy(&nested);
    fs::write(
        dest.join("src/web_audit/vendored/contract/registry.json"),
        "{\"edited\": true}\n",
    )
    .unwrap();
    let output = sync(&site, &dest).arg("--check").output().unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("registry.json"), "{stderr}");
}

#[test]
fn check_names_a_copy_that_is_missing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (site, dest, _) = synced_copy(tmp.path());
    fs::remove_dir_all(dest.join("tests/fixtures/web-audit-contract")).unwrap();
    let output = sync(&site, &dest).arg("--check").output().unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("web-audit-contract"), "{stderr}");
}

/// Reaches the site repository; set `WEB_AUDIT_SYNC_ONLINE=1` to run it.
#[test]
#[ignore = "fetches the pinned commit from GitHub"]
fn the_committed_copies_match_the_pin_over_the_network() {
    let output = Command::new("bash")
        .arg(repo_root().join("scripts/sync-web-audit.sh"))
        .arg("--check")
        .output()
        .expect("run the sync script");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("ok:"), "{stdout}");
}
