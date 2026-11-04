//! The decisions of `scripts/web-audit-bump.sh` and the GitHub calls of
//! `scripts/web-audit-bump-publish.sh`, run for real over a throwaway site
//! repository and a throwaway copy of this repository's scripts. Shims on
//! `PATH` stand in for `cargo` and `gh`, so nothing compiles or reaches the
//! network, and they log every call so a test can say what was not done.
//! The real `sync-web-audit.sh` and `web-audit-divergence-ledger.py` run.
//! The release gate over the same ledger, in `scripts/release/smoke.sh`, is
//! here too.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const LEDGER: &str = "tests/fixtures/web-audit-divergences.toml";
const PIN_FILE: &str = "src/web_audit/vendored/SITE_SHA";
const REGISTRY: &str = "src/web_audit/vendored/contract/registry.json";
/// Where the corpus lands in this repository.
const CORPUS: &str = "tests/fixtures/web-audit-contract/conformance/scenarios";
/// Where the site keeps it, and where the ledger generator reads its history.
const SITE_CORPUS: &str = "src/data/web-audit/contract/test/conformance/scenarios";
const SCRIPTS: [&str; 6] = [
    "sync-web-audit.sh",
    "hardened-git.sh",
    "web-audit-bump.sh",
    "web-audit-bump-publish.sh",
    "web-audit-bump-body.py",
    "web-audit-divergence-ledger.py",
];
/// The site paths the sync script reads, as fixture files.
const SITE_FILES: [&str; 3] = [
    "src/data/web-audit/contract/build/registry.json",
    "src/data/web-audit/contract/test/score-parity.json",
    "src/data/web-audit/contract/test/conformance/scenarios/run-a/scorecard.json",
];

const CARGO_SHIM: &str = r#"#!/usr/bin/env bash
printf 'cargo %s\n' "$*" >>"$SHIM_DIR/calls"
if [[ -n ${WEB_AUDIT_DIVERGENCE_OUT:-} ]]; then
  cat "$SHIM_DIR/divergences.json" >"$WEB_AUDIT_DIVERGENCE_OUT"
fi
if [[ -n ${WEB_AUDIT_UNPORTED_OUT:-} ]]; then
  cat "$SHIM_DIR/unported.json" >"$WEB_AUDIT_UNPORTED_OUT"
fi
if [[ " $* " == *" --lib "* ]]; then
  cat "$SHIM_DIR/suite-output"
  exit "$(cat "$SHIM_DIR/suite-status")"
fi
"#;

/// Answers each endpoint the scripts read from a file the test may have
/// written, the way `gh api --jq` would print the selected value.
const GH_SHIM: &str = r#"#!/usr/bin/env bash
printf 'gh %s\n' "$*" >>"$SHIM_DIR/calls"
args="$*"
case "$args" in
  *"api graphql --input "*)
    cp "${@: -1}" "$SHIM_DIR/payload.json"
    cat "$SHIM_DIR/graphql-response"
    ;;
  *"contents/"*)
    [[ -f $SHIM_DIR/branch-pin ]] || exit 1
    cat "$SHIM_DIR/branch-pin"
    ;;
  *"pulls?head="*) cat "$SHIM_DIR/open-pr" ;;
  *"git/ref/heads/"*)
    [[ -f $SHIM_DIR/branch-head ]] || exit 1
    cat "$SHIM_DIR/branch-head"
    ;;
  *"--jq .parents[0].sha"*) cat "$SHIM_DIR/branch-base" ;;
  *"git/commits/"*) cat "$SHIM_DIR/branch-tree" ;;
  *"--jq .files[].filename"*) cat "$SHIM_DIR/compare-files" ;;
  *"compare/"*) cat "$SHIM_DIR/ahead-by" ;;
  *"--jq .commit.verification.verified"*) cat "$SHIM_DIR/verified" ;;
  *"--jq .html_url"*) echo "https://example.invalid/pr" ;;
esac
"#;

fn run_git(dir: &Path, args: &[&str]) -> String {
    let output = git_command(dir)
        .args(args)
        .output()
        .expect("git runs in the fixture");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn git_command(dir: &Path) -> Command {
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
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null");
    scrub_repo_env(&mut cmd);
    cmd
}

/// A commit or push runs this suite from a hook with variables that pin git
/// to the repository being committed, which would aim every fixture repository
/// at the real one.
fn scrub_repo_env(cmd: &mut Command) {
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_PREFIX",
        "GIT_NAMESPACE",
    ] {
        cmd.env_remove(var);
    }
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

struct Fixture {
    _tmp: tempfile::TempDir,
    site: PathBuf,
    repo: PathBuf,
    shims: PathBuf,
    first_pin: String,
}

impl Fixture {
    /// A site repository on `dev` and a repository pinned to its only commit,
    /// with an empty ledger and a suite that passes.
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let site = tmp.path().join("site");
        let repo = tmp.path().join("repo");
        let shims = tmp.path().join("shims");
        fs::create_dir_all(&site).unwrap();
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&shims).unwrap();

        run_git(&site, &["init", "--quiet", "--initial-branch=dev"]);
        for file in SITE_FILES {
            write(&site.join(file), &format!("{file}: first\n"));
        }
        write(&site.join(SITE_FILES[2]), "{\"score\": 1}\n");
        let first_pin = commit(&site, "feat: the first site commit (#1)");

        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for script in SCRIPTS {
            fs::create_dir_all(repo.join("scripts")).unwrap();
            fs::copy(
                root.join("scripts").join(script),
                repo.join("scripts").join(script),
            )
            .unwrap();
        }
        write(&repo.join(LEDGER), &empty_ledger(&root));
        run_git(&repo, &["init", "--quiet", "--initial-branch=dev"]);
        let mut sync = Command::new("bash");
        sync.current_dir(&repo)
            .args(["scripts/sync-web-audit.sh", "--pin", &first_pin])
            .env("WEB_AUDIT_SITE_REPO", &site);
        scrub_repo_env(&mut sync);
        let synced = sync.output().unwrap();
        assert!(
            synced.status.success(),
            "{}",
            String::from_utf8_lossy(&synced.stderr)
        );
        commit(&repo, "chore: the pinned repository");

        for (name, body) in [("cargo", CARGO_SHIM), ("gh", GH_SHIM)] {
            let shim = shims.join(name);
            fs::write(&shim, body).unwrap();
            fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let fixture = Fixture {
            _tmp: tmp,
            site,
            repo,
            shims,
            first_pin,
        };
        for (file, content) in [
            ("calls", ""),
            ("divergences.json", "[]"),
            ("unported.json", "{\"handlers\": {}, \"evals\": {}}"),
            ("suite-output", "test result: ok. 1 passed; 0 failed\n"),
            ("suite-status", "0"),
            ("open-pr", ""),
            ("ahead-by", "1"),
            ("verified", "true"),
            (
                "graphql-response",
                "{\"data\":{\"createCommitOnBranch\":{\"commit\":{\"oid\":\"newoid\"}}}}",
            ),
        ] {
            fixture.answer(file, content);
        }
        fixture
    }

    fn answer(&self, file: &str, content: &str) {
        fs::write(self.shims.join(file), content).unwrap();
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.shims.join("calls")).unwrap()
    }

    /// Commits a change to one file on the site's `dev`.
    fn site_commit(&self, file: &str, content: &str, subject: &str) -> String {
        write(&self.site.join(file), content);
        commit(&self.site, subject)
    }

    fn bump_command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("bash");
        cmd.current_dir(&self.repo)
            .arg("scripts/web-audit-bump.sh")
            .args(args)
            .env("SHIM_DIR", &self.shims)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.shims.display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("WEB_AUDIT_SITE_REPO", &self.site)
            .env("GITHUB_REPOSITORY", "owner/repo")
            .env_remove("GITHUB_OUTPUT")
            .env_remove("GITHUB_RUN_ID");
        scrub_repo_env(&mut cmd);
        cmd
    }

    fn bump(&self, args: &[&str]) -> Output {
        self.bump_command(args)
            .output()
            .expect("run the bump script")
    }

    /// What `sync-web-audit.sh --pin <site dev>` would commit, as a tree id.
    fn synced_tree(&self) -> String {
        let head = run_git(&self.site, &["rev-parse", "dev"]);
        let mut sync = Command::new("bash");
        sync.current_dir(&self.repo)
            .args(["scripts/sync-web-audit.sh", "--pin", &head])
            .env("WEB_AUDIT_SITE_REPO", &self.site);
        scrub_repo_env(&mut sync);
        assert!(sync.output().unwrap().status.success());
        run_git(&self.repo, &["add", "-A"]);
        let tree = run_git(&self.repo, &["write-tree"]);
        run_git(&self.repo, &["reset", "--quiet", "--hard", "HEAD"]);
        tree
    }

    fn assert_tree_clean(&self) {
        assert_eq!(run_git(&self.repo, &["status", "--porcelain"]), "");
    }

    /// The decoded content the mutation adds at `path`, if it adds one.
    fn added(&self, path: &str) -> Option<String> {
        let payload = self.payload();
        let additions = payload["variables"]["input"]["fileChanges"]["additions"]
            .as_array()
            .expect("an additions list");
        additions
            .iter()
            .find(|a| a["path"] == path)
            .map(|a| decode_base64(a["contents"].as_str().unwrap()))
    }

    fn payload(&self) -> Value {
        let raw = fs::read_to_string(self.shims.join("payload.json")).expect("the mutation ran");
        serde_json::from_str(&raw).unwrap()
    }
}

/// What the generator writes when nothing diverges, so an empty regeneration
/// leaves the fixture's ledger untouched as it does the real one.
fn empty_ledger(root: &Path) -> String {
    let generator =
        fs::read_to_string(root.join("scripts/web-audit-divergence-ledger.py")).unwrap();
    let (_, rest) = generator.split_once("HEADER = \"\"\"\\\n").unwrap();
    let (header, _) = rest.split_once("\"\"\"").unwrap();
    header.to_string()
}

fn commit(dir: &Path, subject: &str) -> String {
    run_git(dir, &["add", "-A"]);
    run_git(dir, &["commit", "--quiet", "-m", subject]);
    run_git(dir, &["rev-parse", "HEAD"])
}

fn decode_base64(encoded: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = 0u32;
    let mut count = 0;
    let mut out = Vec::new();
    for byte in encoded.bytes().filter(|b| *b != b'=') {
        let value = ALPHABET.iter().position(|c| *c == byte).unwrap() as u32;
        bits = (bits << 6) | value;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn assert_ok(output: &Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        stdout(output),
        String::from_utf8_lossy(&output.stderr)
    );
}

// The decision.

#[test]
fn the_current_pin_has_nothing_to_bump() {
    let f = Fixture::new();
    let output = f.bump(&["--dry-run"]);
    assert_ok(&output);
    assert!(
        stdout(&output).contains("nothing to bump"),
        "{}",
        stdout(&output)
    );
    assert_eq!(f.calls(), "", "no toolchain and no GitHub call");
    f.assert_tree_clean();
}

#[test]
fn a_site_move_that_touches_no_vendored_file_leaves_the_pin_alone() {
    let f = Fixture::new();
    f.site_commit("README.md", "not vendored\n", "docs: a readme (#2)");
    let output = f.bump(&[]);
    assert_ok(&output);
    assert!(
        stdout(&output).contains("changes no vendored file"),
        "{}",
        stdout(&output)
    );
    assert_eq!(f.calls(), "", "no toolchain and no GitHub call");
    f.assert_tree_clean();
    assert_eq!(
        fs::read_to_string(f.repo.join(PIN_FILE)).unwrap().trim(),
        f.first_pin
    );
}

#[test]
fn a_dirty_vendored_path_refuses_to_start() {
    let f = Fixture::new();
    write(&f.repo.join(REGISTRY), "edited by hand\n");
    let output = f.bump(&["--dry-run"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("vendored paths"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(f.repo.join(REGISTRY)).unwrap(),
        "edited by hand\n"
    );
}

#[test]
fn decide_answers_without_a_toolchain() {
    let f = Fixture::new();
    let output_file = f.shims.join("github-output");
    let decide = |output_file: &Path| {
        f.bump_command(&["--decide"])
            .env("GITHUB_OUTPUT", output_file)
            .output()
            .unwrap()
    };
    assert!(stdout(&decide(&output_file)).contains("bump=no"));
    assert_eq!(fs::read_to_string(&output_file).unwrap(), "bump=no\n");

    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    fs::remove_file(&output_file).unwrap();
    assert!(stdout(&decide(&output_file)).contains("bump=yes"));
    assert_eq!(fs::read_to_string(&output_file).unwrap(), "bump=yes\n");

    assert_eq!(f.calls(), "", "no toolchain and no GitHub call");
    f.assert_tree_clean();
}

#[test]
fn a_dev_push_leaves_an_open_pr_alone_unless_it_touched_what_the_suites_read() {
    let f = Fixture::new();
    let head = f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");

    let no_branch = f.bump(&["--decide", "--on-dev-push"]);
    assert!(
        stdout(&no_branch).contains("bump=yes"),
        "{}",
        stdout(&no_branch)
    );

    f.answer("branch-pin", &f.first_pin);
    let older = f.bump(&["--decide", "--on-dev-push"]);
    assert!(stdout(&older).contains("bump=yes"), "{}", stdout(&older));

    f.answer("branch-pin", &format!("{head}\n"));
    f.answer("branch-head", "bothead");
    f.answer("branch-base", "botbase");
    f.answer("compare-files", "docs/plans/a-plan.md\nREADME.md\n");
    let carried = f.bump(&["--decide", "--on-dev-push"]);
    assert!(stdout(&carried).contains("bump=no"), "{}", stdout(&carried));
    assert!(
        stdout(&carried).contains("already carries"),
        "{}",
        stdout(&carried)
    );
    assert!(!f.calls().contains("cargo"), "{}", f.calls());

    for touched in [
        "src/web_audit/mod.rs",
        "tests/web_audit_cli.rs",
        "scripts/sync-web-audit.sh",
        "build.rs",
        "Cargo.lock",
    ] {
        f.answer("compare-files", &format!("README.md\n{touched}\n"));
        let moved = f.bump(&["--decide", "--on-dev-push"]);
        assert!(
            stdout(&moved).contains("bump=yes"),
            "{touched}: {}",
            stdout(&moved)
        );
    }

    // GitHub could not say what changed, so the suites run.
    fs::remove_file(f.shims.join("branch-head")).unwrap();
    let unknown = f.bump(&["--decide", "--on-dev-push"]);
    assert!(
        stdout(&unknown).contains("bump=yes"),
        "{}",
        stdout(&unknown)
    );

    let refresh = f.bump(&["--decide"]);
    assert!(
        stdout(&refresh).contains("bump=yes"),
        "{}",
        stdout(&refresh)
    );
    f.assert_tree_clean();
}

#[test]
fn new_pin_only_is_a_decide_flag() {
    let f = Fixture::new();
    assert_eq!(f.bump(&["--on-dev-push"]).status.code(), Some(2));
}

// The report.

#[test]
fn a_dry_run_reports_the_bump_and_restores_the_tree() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    let new = f.site_commit(
        &format!("{SITE_CORPUS}/run-b/scorecard.json"),
        "{\"score\": 2}\n",
        "feat: a new scenario (#3)",
    );
    let output = f.bump(&["--dry-run"]);
    assert_ok(&output);
    let out = stdout(&output);

    let title = format!(
        "title: feat(web): re-pin to agentnative-site dev {}\n",
        &new[..7]
    );
    assert!(out.starts_with(&title), "{out}");
    assert!(
        out.contains(&format!("commit/{}", f.first_pin)),
        "old pin: {out}"
    );
    assert!(out.contains(&format!("commit/{new}")), "new pin: {out}");
    assert!(out.contains(&format!("- `{REGISTRY}`")), "{out}");
    assert!(
        out.contains(&format!("- `{CORPUS}/run-b/scorecard.json`")),
        "{out}"
    );
    assert!(
        !out.contains(&format!("- `{PIN_FILE}`")),
        "the pin is not an input: {out}"
    );
    assert!(
        out.contains("None: the engine reproduces the whole corpus"),
        "{out}"
    );
    assert!(out.contains("None: this build runs every check"), "{out}");
    assert!(out.contains("Passing at this pin"), "{out}");
    assert!(out.contains("## Changelog"), "{out}");

    let calls = f.calls();
    assert!(
        calls.contains("cargo test --features web --no-fail-fast --lib"),
        "{calls}"
    );
    assert!(
        !calls.contains("gh "),
        "a dry run reaches no GitHub: {calls}"
    );
    f.assert_tree_clean();
    assert!(!f.repo.join(format!("{CORPUS}/run-b")).exists());
}

#[test]
fn deleted_vendored_files_are_called_out_because_no_test_can_see_a_shrink() {
    let f = Fixture::new();
    run_git(
        &f.site,
        &[
            "rm",
            "--quiet",
            "--",
            &format!("{SITE_CORPUS}/run-a/scorecard.json"),
        ],
    );
    f.site_commit(
        &format!("{SITE_CORPUS}/run-b/scorecard.json"),
        "{\"score\": 2}\n",
        "feat: a scenario replaced (#3)",
    );
    let output = f.bump(&["--dry-run"]);
    assert_ok(&output);
    let out = stdout(&output);
    assert!(out.contains("**1 of these are deleted.**"), "{out}");
    let list = out.split("are deleted.**").nth(1).unwrap();
    assert!(
        list.contains(&format!("- `{CORPUS}/run-a/scorecard.json`")),
        "{out}"
    );
    f.assert_tree_clean();
}

#[test]
fn a_failing_suite_still_bumps_under_a_title_that_says_so() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("suite-status", "101");
    f.answer(
        "suite-output",
        "test parses_the_registry ... FAILED\n\
         run-a/row:robots: diverges from the site and has no ledger entry\n\
         test result: FAILED. 0 passed; 1 failed\n",
    );
    let output = f.bump(&["--dry-run"]);
    assert_ok(&output);
    let out = stdout(&output);
    assert!(
        out.lines()
            .next()
            .unwrap()
            .ends_with("(web suites fail, port needed)"),
        "{out}"
    );
    assert!(out.contains("- `parses_the_registry`"), "{out}");
    assert!(
        out.contains("- `run-a/row:robots: diverges from the site"),
        "{out}"
    );
    assert!(!out.contains("Passing at this pin"), "{out}");
}

#[test]
fn a_suite_that_cannot_build_says_so() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("suite-status", "101");
    f.answer(
        "suite-output",
        "error: unknown registry key `frob` on check robots\n",
    );
    let output = f.bump(&["--dry-run"]);
    assert_ok(&output);
    let out = stdout(&output);
    assert!(
        out.lines()
            .next()
            .unwrap()
            .ends_with("(web suites fail, port needed)"),
        "{out}"
    );
    assert!(out.contains("The suites did not build"), "{out}");
    assert!(out.contains("unknown registry key `frob`"), "{out}");
}

#[test]
fn divergences_land_green_as_ledger_entries_grouped_by_owning_site_pr() {
    let f = Fixture::new();
    f.site_commit(
        &format!("{SITE_CORPUS}/run-a/scorecard.json"),
        "{\"score\": 2, \"results\": [{\"id\": \"robots\", \"status\": \"pass\"}]}\n",
        "feat: a golden moves (#9001)",
    );
    f.answer(
        "divergences.json",
        "[[\"run-a\", \"row:robots\"], [\"run-a\", \"score\"]]",
    );
    f.answer(
        "unported.json",
        "{\"handlers\": {\"frobnicate\": [\"robots\"]}, \"evals\": {\"quux\": [\"robots\", \"llms-txt\"]}}",
    );
    let output = f.bump(&["--dry-run"]);
    assert_ok(&output);
    let out = stdout(&output);
    assert!(
        !out.lines().next().unwrap().contains("fail"),
        "green title: {out}"
    );
    assert!(out.contains("holds 2 entries"), "{out}");
    assert!(
        out.contains("| brettdavies/agentnative-site#9001 | 2 |"),
        "{out}"
    );
    assert!(
        out.contains("brettdavies/agentnative-site#9001: `run-a/row:robots`"),
        "{out}"
    );
    assert!(out.contains("- handler `frobnicate`: `robots`"), "{out}");
    assert!(out.contains("- eval `quux`: `robots`, `llms-txt`"), "{out}");
    f.assert_tree_clean();
}

#[test]
fn a_standing_entry_keeps_its_owner_across_a_later_bump() {
    let f = Fixture::new();
    write(
        &f.repo.join(LEDGER),
        &format!(
            "site_sha = \"{}\"\n\n[[divergence]]\nscenario = \"run-a\"\nelement = \"score\"\nowner = \"#77\"\n",
            f.first_pin
        ),
    );
    commit(&f.repo, "chore: a standing entry");
    let new = f.site_commit(
        SITE_FILES[0],
        "moved\n",
        "feat: nothing to do with run-a (#9002)",
    );
    f.answer("divergences.json", "[[\"run-a\", \"score\"]]");
    let output = f.bump(&[]);
    assert_ok(&output);
    let ledger = f.added(LEDGER).expect("the ledger moved to the new pin");
    assert!(
        ledger.contains(&format!("site_sha = \"{new}\"")),
        "{ledger}"
    );
    assert!(ledger.contains("owner = \"#77\""), "{ledger}");
    assert!(!ledger.contains("#9002"), "{ledger}");
}

#[test]
fn a_divergence_with_no_attributable_site_change_fails_visibly() {
    let f = Fixture::new();
    f.site_commit(
        SITE_FILES[0],
        "moved\n",
        "feat: nothing to do with run-a (#9002)",
    );
    f.answer("divergences.json", "[[\"run-a\", \"score\"]]");
    f.answer("suite-status", "101");
    f.answer(
        "suite-output",
        "run-a/score: diverges from the site and has no ledger entry\ntest result: FAILED. 0 passed; 1 failed\n",
    );
    let output = f.bump(&["--dry-run"]);
    assert_ok(&output);
    let out = stdout(&output);
    assert!(
        out.lines()
            .next()
            .unwrap()
            .ends_with("(web suites fail, port needed)"),
        "{out}"
    );
    assert!(out.contains("The ledger did not regenerate"), "{out}");
    assert!(out.contains("no site PR owns run-a/score"), "{out}");
}

// Publishing.

#[test]
fn a_first_publish_signs_on_a_scratch_ref_then_creates_the_branch_and_the_pr() {
    let f = Fixture::new();
    let base = run_git(&f.repo, &["rev-parse", "HEAD"]);
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    run_git(
        &f.site,
        &[
            "rm",
            "--quiet",
            "--",
            &format!("{SITE_CORPUS}/run-a/scorecard.json"),
        ],
    );
    let new = f.site_commit(
        &format!("{SITE_CORPUS}/run-b/scorecard.json"),
        "{\"score\": 2}\n",
        "feat: a scenario replaced (#3)",
    );
    let output = f.bump(&[]);
    assert_ok(&output);
    f.assert_tree_clean();

    let calls = f.calls();
    let position = |needle: &str| {
        calls
            .find(needle)
            .unwrap_or_else(|| panic!("no {needle} in:\n{calls}"))
    };
    let scratch = position(&format!(
        "api -X POST repos/owner/repo/git/refs -f ref=refs/heads/bot/web-audit-bump-next -f sha={base}"
    ));
    let mutation = position("api graphql --input");
    let verify = position("repos/owner/repo/commits/newoid --jq .commit.verification.verified");
    let branch = position(
        "api -X POST repos/owner/repo/git/refs -f ref=refs/heads/bot/web-audit-bump -f sha=newoid",
    );
    let pr = position("api -X POST repos/owner/repo/pulls");
    assert!(
        scratch < mutation && mutation < verify && verify < branch && branch < pr,
        "{calls}"
    );
    assert!(
        calls.ends_with("api -X DELETE repos/owner/repo/git/refs/heads/bot/web-audit-bump-next\n"),
        "{calls}"
    );
    assert!(
        calls.contains("-f head=bot/web-audit-bump -f base=dev"),
        "{calls}"
    );

    let payload = f.payload();
    let input = &payload["variables"]["input"];
    assert_eq!(input["expectedHeadOid"], Value::from(base));
    assert_eq!(input["branch"]["branchName"], "bot/web-audit-bump-next");
    assert_eq!(f.added(REGISTRY).as_deref(), Some("moved\n"));
    assert_eq!(f.added(PIN_FILE).map(|p| p.trim().to_string()), Some(new));
    assert_eq!(
        f.added(&format!("{CORPUS}/run-b/scorecard.json"))
            .as_deref(),
        Some("{\"score\": 2}\n")
    );
    assert_eq!(f.added(LEDGER), None, "an empty ledger stays as it was");
    let deletions = input["fileChanges"]["deletions"].as_array().unwrap();
    assert_eq!(deletions.len(), 1, "{deletions:?}");
    assert_eq!(
        deletions[0]["path"],
        format!("{CORPUS}/run-a/scorecard.json")
    );
}

#[test]
fn a_regenerated_ledger_lands_in_the_commit_with_its_pin() {
    let f = Fixture::new();
    let new = f.site_commit(
        &format!("{SITE_CORPUS}/run-a/scorecard.json"),
        "{\"score\": 2}\n",
        "feat: a golden moves (#9001)",
    );
    f.answer("divergences.json", "[[\"run-a\", \"score\"]]");
    assert_ok(&f.bump(&[]));
    let ledger = f.added(LEDGER).expect("the ledger moved");
    assert!(
        ledger.contains(&format!("site_sha = \"{new}\"")),
        "{ledger}"
    );
    assert!(ledger.contains("owner = \"#9001\""), "{ledger}");
}

#[test]
fn an_open_pr_is_force_updated_and_edited() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("open-pr", "42");
    f.answer("branch-head", "oldhead");
    f.answer("branch-tree", "oldtree");
    assert_ok(&f.bump(&[]));
    let calls = f.calls();
    assert!(
        calls.contains("api -X PATCH repos/owner/repo/git/refs/heads/bot/web-audit-bump -f sha=newoid -F force=true"),
        "{calls}"
    );
    assert!(
        calls.contains("api -X PATCH repos/owner/repo/pulls/42 -f title="),
        "{calls}"
    );
    assert!(
        !calls.contains("api -X POST repos/owner/repo/pulls"),
        "{calls}"
    );
}

#[test]
fn the_same_tree_pushes_nothing_and_still_edits_the_pr() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    let tree = f.synced_tree();
    f.answer("open-pr", "42");
    f.answer("branch-head", "oldhead");
    f.answer("branch-tree", &tree);
    let output = f.bump(&[]);
    assert_ok(&output);
    assert!(
        stdout(&output).contains("already holds this tree"),
        "{}",
        stdout(&output)
    );
    let calls = f.calls();
    assert!(!calls.contains("graphql"), "{calls}");
    assert!(!calls.contains("git/refs"), "{calls}");
    assert!(
        calls.contains("api -X PATCH repos/owner/repo/pulls/42"),
        "{calls}"
    );
}

#[test]
fn port_commits_on_the_branch_make_the_bot_stand_down() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("open-pr", "42");
    f.answer("branch-head", "porthead");
    f.answer("ahead-by", "2");
    let output = f.bump(&[]);
    assert_ok(&output);
    assert!(
        stdout(&output).contains("standing down"),
        "{}",
        stdout(&output)
    );
    let calls = f.calls();
    assert!(!calls.contains("graphql"), "{calls}");
    assert!(
        !calls.contains("-X PATCH"),
        "the branch and the PR keep their state: {calls}"
    );
    assert!(!calls.contains("-X POST"), "{calls}");
}

#[test]
fn a_graphql_error_fails_the_run_and_opens_nothing() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer(
        "graphql-response",
        "{\"errors\": [{\"message\": \"expectedHeadOid moved\"}]}",
    );
    let output = f.bump(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("expectedHeadOid moved"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = f.calls();
    assert!(
        !calls.contains("refs/heads/bot/web-audit-bump -f sha=newoid"),
        "{calls}"
    );
    assert!(!calls.contains("repos/owner/repo/pulls -f"), "{calls}");
    assert!(
        calls.ends_with("api -X DELETE repos/owner/repo/git/refs/heads/bot/web-audit-bump-next\n"),
        "{calls}"
    );
    f.assert_tree_clean();
}

#[test]
fn an_unverified_commit_never_reaches_the_branch() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("verified", "false");
    let output = f.bump(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not verified"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!f.calls().contains("sha=newoid"), "{}", f.calls());
}

// A pin off site dev.

impl Fixture {
    /// Pins the repository to a commit on a side branch of the site, as a pin to a
    /// site PR's head does, with the same vendored bytes dev holds.
    fn pin_to_a_site_pr_head(&self) -> String {
        run_git(&self.site, &["switch", "--quiet", "-c", "pr"]);
        let head = self.site_commit("PR.md", "not vendored\n", "feat: a PR commit");
        run_git(&self.site, &["switch", "--quiet", "dev"]);
        let mut sync = Command::new("bash");
        sync.current_dir(&self.repo)
            .args(["scripts/sync-web-audit.sh", "--pin", &head])
            .env("WEB_AUDIT_SITE_REPO", &self.site);
        scrub_repo_env(&mut sync);
        assert!(sync.output().unwrap().status.success());
        commit(&self.repo, "chore: pin to the PR's head");
        self.site_commit("OTHER.md", "not vendored\n", "docs: dev moves on (#3)");
        head
    }
}

#[test]
fn a_pin_off_site_dev_always_moves_even_when_the_bytes_match() {
    let f = Fixture::new();
    let pr_head = f.pin_to_a_site_pr_head();
    let dev = run_git(&f.site, &["rev-parse", "dev"]);

    let decide = f.bump(&["--decide"]);
    assert!(stdout(&decide).contains("bump=yes"), "{}", stdout(&decide));

    let output = f.bump(&["--dry-run"]);
    assert_ok(&output);
    let out = stdout(&output);
    let title = format!(
        "title: feat(web): re-pin to agentnative-site dev {}\n",
        &dev[..7]
    );
    assert!(out.starts_with(&title), "{out}");
    assert!(out.contains("None: the bytes match."), "{out}");
    assert!(out.contains(&pr_head[..7]), "{out}");
    f.assert_tree_clean();

    assert_ok(&f.bump(&[]));
    assert_eq!(f.added(PIN_FILE).map(|p| p.trim().to_string()), Some(dev));
    assert_eq!(f.added(REGISTRY), None, "only the pin moves");
}

#[test]
fn a_pin_already_on_site_dev_with_the_same_bytes_stays_put() {
    let f = Fixture::new();
    f.site_commit("OTHER.md", "not vendored\n", "docs: dev moves on (#3)");
    let output = f.bump(&["--dry-run"]);
    assert!(
        stdout(&output).contains("nothing to bump"),
        "{}",
        stdout(&output)
    );
}

// Preparing without the token.

impl Fixture {
    /// What the workflow's second job holds: a fresh checkout of the base, with the
    /// first job's output beside it.
    fn second_job_checkout(&self) -> PathBuf {
        let checkout = self.shims.join("second-job");
        run_git(
            &self.shims,
            &[
                "clone",
                "--quiet",
                self.repo.to_str().unwrap(),
                checkout.to_str().unwrap(),
            ],
        );
        checkout
    }

    fn publish_in(&self, checkout: &Path, prepared: &Path) -> Output {
        let mut publish = Command::new("bash");
        publish
            .current_dir(checkout)
            .arg("scripts/web-audit-bump-publish.sh")
            .arg(prepared)
            .env("SHIM_DIR", &self.shims)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.shims.display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("GITHUB_REPOSITORY", "owner/repo");
        scrub_repo_env(&mut publish);
        publish.output().unwrap()
    }

    fn prepare(&self) -> PathBuf {
        let prepared = self.shims.join("prepared");
        assert_ok(&self.bump(&["--prepare", prepared.to_str().unwrap()]));
        prepared
    }
}

#[test]
fn prepare_runs_the_suites_with_no_github_call_and_a_second_job_lands_what_it_left() {
    let f = Fixture::new();
    let new = f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    let base = run_git(&f.repo, &["rev-parse", "HEAD"]);
    let prepared = f.shims.join("prepared");
    let output_file = f.shims.join("github-output");
    let mut prepare = f.bump_command(&["--prepare", prepared.to_str().unwrap()]);
    prepare.env("GITHUB_OUTPUT", &output_file);
    let output = prepare.output().unwrap();
    assert_ok(&output);
    assert_eq!(
        fs::read_to_string(&output_file).unwrap(),
        format!("ready=yes\nbase={base}\n")
    );
    let calls = f.calls();
    assert!(calls.contains("cargo test"), "{calls}");
    assert!(!calls.contains("gh "), "no token is needed yet: {calls}");

    let checkout = f.second_job_checkout();
    assert_ok(&f.publish_in(&checkout, &prepared));
    assert_eq!(f.added(REGISTRY).as_deref(), Some("moved\n"));
    assert_eq!(f.added(PIN_FILE).map(|p| p.trim().to_string()), Some(new));
    let calls = f.calls();
    assert!(
        calls.contains("-f head=bot/web-audit-bump -f base=dev"),
        "{calls}"
    );
    assert!(calls.contains(&format!("-f sha={base}")), "{calls}");
}

#[test]
fn publish_commits_only_the_allowlist_however_the_first_job_filled_the_tree() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    let prepared = f.prepare();
    for planted in [
        ".github/workflows/evil.yml",
        "src/main.rs",
        "scripts/web-audit-bump.sh",
    ] {
        write(&prepared.join("tree").join(planted), "planted\n");
    }
    let checkout = f.second_job_checkout();
    assert_ok(&f.publish_in(&checkout, &prepared));
    let payload = f.payload();
    let additions = payload["variables"]["input"]["fileChanges"]["additions"]
        .as_array()
        .unwrap();
    let paths: Vec<&str> = additions
        .iter()
        .map(|a| a["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths.len(), 2, "{paths:?}");
    assert!(
        paths.contains(&REGISTRY) && paths.contains(&PIN_FILE),
        "{paths:?}"
    );
}

#[test]
fn publish_ignores_a_branch_the_first_job_named() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    let prepared = f.prepare();
    write(&prepared.join("branch"), "main\n");
    let checkout = f.second_job_checkout();
    assert_ok(&f.publish_in(&checkout, &prepared));
    let calls = f.calls();
    assert!(
        calls.contains("refs/heads/bot/web-audit-bump -f sha=newoid"),
        "{calls}"
    );
    assert!(!calls.contains("heads/main"), "{calls}");
}

#[test]
fn publish_refuses_a_symlink_in_the_tree() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    let prepared = f.prepare();
    let registry = prepared.join("tree").join(REGISTRY);
    fs::remove_file(&registry).unwrap();
    std::os::unix::fs::symlink("/etc/hostname", &registry).unwrap();
    let checkout = f.second_job_checkout();
    let output = f.publish_in(&checkout, &prepared);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("symlink"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!f.calls().contains("graphql"), "{}", f.calls());
}

#[test]
fn two_bumps_in_a_row_carry_an_entry_no_commit_in_the_second_range_touched() {
    let f = Fixture::new();
    f.site_commit(
        &format!("{SITE_CORPUS}/run-a/scorecard.json"),
        "{\"score\": 2}\n",
        "feat: a golden moves (#9001)",
    );
    f.answer("divergences.json", "[[\"run-a\", \"score\"]]");
    // The first bump lands on dev as the merged PR would.
    let prepared = f.prepare();
    for path in [LEDGER, PIN_FILE] {
        assert!(prepared.join("tree").join(path).exists(), "{path}");
    }
    commit(&f.repo, "feat(web): the first bump");

    let new = f.site_commit(
        SITE_FILES[0],
        "moved\n",
        "feat: nothing to do with run-a (#9002)",
    );
    let output = f.bump(&[]);
    assert_ok(&output);
    let out = stdout(&output);
    assert!(!out.contains("no site PR owns"), "{out}");
    let ledger = f.added(LEDGER).expect("the ledger moved to the new pin");
    assert!(
        ledger.contains(&format!("site_sha = \"{new}\"")),
        "{ledger}"
    );
    assert!(ledger.contains("owner = \"#9001\""), "{ledger}");
    assert!(
        f.calls().contains("api -X POST repos/owner/repo/pulls"),
        "{}",
        f.calls()
    );
}

// The release gate.

const ALL_PORTED: &str =
    r#"{"checks": [{"id": "robots", "handler": "http", "eval": null, "ported": true}]}"#;

/// A tree holding only what the gate reads, and a binary that prints `checks` for
/// `emit web-checks`, or fails when there are none.
struct GateTree {
    tmp: tempfile::TempDir,
    site: PathBuf,
    off_dev: String,
}

impl GateTree {
    fn new(ledger: Option<&str>, web_audit: bool, checks: Option<&str>) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        fs::create_dir_all(tmp.path().join("scripts/release")).unwrap();
        for script in ["smoke.sh", "_lib.sh", "release.env", "web-parity-gate.sh"] {
            let dest = tmp.path().join("scripts/release").join(script);
            fs::copy(root.join("scripts/release").join(script), &dest).unwrap();
        }
        for script in ["sync-web-audit.sh", "hardened-git.sh"] {
            let dest = tmp.path().join("scripts").join(script);
            fs::copy(root.join("scripts").join(script), &dest).unwrap();
        }
        // A site whose dev holds the pin, and a commit off dev to pin to instead.
        let site = tmp.path().join("site");
        fs::create_dir_all(&site).unwrap();
        run_git(&site, &["init", "--quiet", "--initial-branch=dev"]);
        write(&site.join("README.md"), "on dev\n");
        let on_dev = commit(&site, "docs: on dev");
        run_git(&site, &["switch", "--quiet", "-c", "pr"]);
        write(&site.join("PR.md"), "off dev\n");
        let off_dev = commit(&site, "feat: off dev");
        run_git(&site, &["switch", "--quiet", "dev"]);
        if web_audit {
            write(&tmp.path().join(PIN_FILE), &format!("{on_dev}\n"));
        }
        if let Some(ledger) = ledger {
            write(&tmp.path().join(LEDGER), ledger);
        }
        let bin = tmp.path().join("anc");
        let body = match checks {
            Some(checks) => format!("#!/usr/bin/env bash\ncat <<'JSON'\n{checks}\nJSON\n"),
            None => "#!/usr/bin/env bash\nexit 2\n".to_string(),
        };
        fs::write(&bin, body).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        GateTree { tmp, site, off_dev }
    }

    fn pin_off_site_dev(&self) {
        write(
            &self.tmp.path().join(PIN_FILE),
            &format!("{}\n", self.off_dev),
        );
    }

    fn command(&self, script: &str) -> Command {
        let mut cmd = Command::new("bash");
        cmd.arg(self.tmp.path().join("scripts/release").join(script))
            .arg("--bin")
            .arg(self.tmp.path().join("anc"))
            .env("WEB_AUDIT_SITE_REPO", &self.site);
        scrub_repo_env(&mut cmd);
        cmd
    }

    /// The shared script's lines and exit code.
    fn gate(&self) -> (String, Option<i32>) {
        let output = self
            .command("web-parity-gate.sh")
            .output()
            .expect("run the gate");
        (stdout(&output), output.status.code())
    }

    /// `smoke.sh`'s report. Its other gates fail on the stand-in binary, so a test
    /// reads the web gate's lines rather than the exit code.
    fn smoke(&self) -> String {
        let output = self
            .command("smoke.sh")
            .args(["--targets", "no-such-target-on-path"])
            .output()
            .expect("run smoke.sh");
        stdout(&output)
    }
}

const HANDLING_DOCS: &str = "RELEASES.md#handling-a-bump-pr";
const TWO_ENTRIES: &str = "site_sha = \"abc\"\n\n[[divergence]]\nscenario = \"a\"\nelement = \"score\"\nowner = \"#458\"\n\n[[divergence]]\nscenario = \"b\"\nelement = \"score\"\nowner = \"#397\"\n";

#[test]
fn the_gate_passes_an_empty_ledger_and_a_build_that_runs_every_check() {
    let tree = GateTree::new(Some("# header only\n"), true, Some(ALL_PORTED));
    let (out, code) = tree.gate();
    assert_eq!(code, Some(0), "{out}");
    assert!(
        out.contains("pass\tthe divergence ledger has no entries"),
        "{out}"
    );
    assert!(
        out.contains("pass\tevery vendored check runs in this build"),
        "{out}"
    );
    assert!(out.contains("pass\tthe site pin is on site dev"), "{out}");
}

#[test]
fn the_gate_refuses_a_ledger_with_entries_and_names_their_owners() {
    let tree = GateTree::new(Some(TWO_ENTRIES), true, Some(ALL_PORTED));
    let (out, code) = tree.gate();
    assert_eq!(code, Some(1), "{out}");
    assert!(
        out.contains("fail\tweb-audit-ledger: the divergence ledger holds 2 entries"),
        "{out}"
    );
    assert!(out.contains("#397") && out.contains("#458"), "{out}");
    assert!(
        out.contains("Fix: ") && out.contains(HANDLING_DOCS),
        "{out}"
    );
}

#[test]
fn the_gate_refuses_an_unported_check_even_with_an_empty_ledger() {
    let checks = r#"{"checks": [{"id": "robots", "handler": "frobnicate", "eval": null, "ported": false}, {"id": "llms-txt", "handler": "http", "eval": null, "ported": true}]}"#;
    let tree = GateTree::new(Some("# header only\n"), true, Some(checks));
    let (out, code) = tree.gate();
    assert_eq!(code, Some(1), "{out}");
    assert!(
        out.contains("pass\tthe divergence ledger has no entries"),
        "{out}"
    );
    assert!(
        out.contains("fail\tweb-audit-unported: vendored checks bind Unsupported"),
        "{out}"
    );
    assert!(out.contains("robots (handler frobnicate"), "{out}");
    assert!(
        out.contains("Fix: ") && out.contains(HANDLING_DOCS),
        "{out}"
    );
    assert!(!out.contains("llms-txt"), "{out}");
}

#[test]
fn the_gate_reads_a_missing_ported_field_as_unported() {
    let checks = r#"{"checks": [{"id": "robots", "handler": "http", "eval": null}]}"#;
    let tree = GateTree::new(Some("# header only\n"), true, Some(checks));
    let (out, code) = tree.gate();
    assert_eq!(code, Some(1), "{out}");
    assert!(
        out.contains("fail\tweb-audit-unported: vendored checks bind Unsupported"),
        "{out}"
    );
}

#[test]
fn the_gate_fails_when_the_binary_cannot_say() {
    let tree = GateTree::new(Some("# header only\n"), true, None);
    let (out, code) = tree.gate();
    assert_eq!(code, Some(1), "{out}");
    assert!(
        out.contains("fail\tweb-audit-unported: cannot read the checks"),
        "{out}"
    );
    assert!(out.contains("--features web"), "{out}");
}

#[test]
fn the_gate_refuses_a_missing_ledger_while_the_web_audit_ships() {
    let tree = GateTree::new(None, true, Some(ALL_PORTED));
    let (out, code) = tree.gate();
    assert_eq!(code, Some(1), "{out}");
    assert!(
        out.contains("is missing while src/web_audit/ ships"),
        "{out}"
    );
}

#[test]
fn the_gate_refuses_a_pin_off_site_dev_and_says_how_to_move_it() {
    let tree = GateTree::new(Some("# header only\n"), true, Some(ALL_PORTED));
    tree.pin_off_site_dev();
    let (out, code) = tree.gate();
    assert_eq!(code, Some(1), "{out}");
    assert!(
        out.contains("fail\tweb-audit-pin: the site pin is not on site dev"),
        "{out}"
    );
    assert!(out.contains(&tree.off_dev), "{out}");
    assert!(
        out.contains("gh workflow run web-audit-bump.yml --ref dev"),
        "{out}"
    );
    assert!(out.contains(HANDLING_DOCS), "{out}");
}

#[test]
fn the_gate_skips_a_tree_that_strips_the_web_audit_without_reading_the_binary() {
    let tree = GateTree::new(None, false, None);
    let (out, code) = tree.gate();
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("skip\tweb audit parity"), "{out}");
    assert!(out.contains("strips anc web"), "{out}");
}

#[test]
fn smoke_reports_the_gates_conditions_with_its_own_marks() {
    let passing = GateTree::new(Some("# header only\n"), true, Some(ALL_PORTED)).smoke();
    assert!(
        passing.contains("✓ the divergence ledger has no entries"),
        "{passing}"
    );
    assert!(
        passing.contains("✓ every vendored check runs in this build"),
        "{passing}"
    );
    assert!(
        passing.contains("✓ the site pin is on site dev"),
        "{passing}"
    );
    let failing = GateTree::new(Some(TWO_ENTRIES), true, Some(ALL_PORTED)).smoke();
    assert!(
        failing.contains("✗ web-audit-ledger: the divergence ledger holds 2 entries"),
        "{failing}"
    );
    let stripped = GateTree::new(None, false, Some(ALL_PORTED)).smoke();
    assert!(stripped.contains("⊝ web audit parity"), "{stripped}");
}
