//! The decisions of `scripts/web-audit-bump.sh` and the GitHub calls of
//! `scripts/web-audit-bump-publish.sh`, run for real over a throwaway site
//! repository and a throwaway copy of this repository's scripts. Shims on
//! `PATH` stand in for `cargo` and `gh`, so nothing compiles or reaches the
//! network, and they log every call so a test can say what was not done.
//! The real `sync-web-audit.sh` and `web-audit-divergence-ledger.py` run.
//! The release gate over the same ledger, in `scripts/release/smoke.sh`, is
//! here too.

#![cfg(unix)]

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

const LEDGER: &str = "tests/fixtures/web-audit-divergences.toml";
const PIN_FILE: &str = "src/web_audit/vendored/SITE_SHA";
const REGISTRY: &str = "src/web_audit/vendored/registry.yaml";
const CORPUS: &str = "tests/fixtures/web-audit-conformance/scenarios";
const SCRIPTS: [&str; 7] = [
    "sync-web-audit.sh",
    "hardened-git.sh",
    "web-audit-bump.sh",
    "web-audit-bump-publish.sh",
    "web-audit-bump-body.py",
    "web-audit-divergence-ledger.py",
    "cargo-inputs.sh",
];
/// The site paths the sync script reads, as fixture files.
const SITE_FILES: [&str; 8] = [
    "src/data/web-audit/registry.yaml",
    "src/data/web-audit/remediation.yaml",
    "src/data/web-audit/server-card.schema.json",
    "src/shared/user-agents.ts",
    "src/shared/site-url.ts",
    "src/shared/audit-routes.ts",
    "tests/fixtures/web-audit-score-parity.json",
    "tests/fixtures/web-audit-conformance/scenarios/run-a/scorecard.json",
];

const CARGO_SHIM: &str = r#"#!/usr/bin/env bash
printf 'cargo %s\n' "$*" >>"$SHIM_DIR/calls"
if [[ $1 == fetch ]]; then
  exit "$(cat "$SHIM_DIR/fetch-status")"
fi
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
    if grep -q updateRefs "${@: -1}"; then
      cp "${@: -1}" "$SHIM_DIR/update-refs.json"
      cat "$SHIM_DIR/update-refs-response"
    else
      cp "${@: -1}" "$SHIM_DIR/payload.json"
      n="$(find "$SHIM_DIR" -maxdepth 1 -name 'commit-*.json' | wc -l | tr -d ' ')"
      cp "${@: -1}" "$SHIM_DIR/commit-$n.json"
      if [[ $n == "$(cat "$SHIM_DIR/fail-at-commit" 2>/dev/null)" ]]; then
        printf '{"errors": [{"message": "commit %s failed"}]}' "$n"
      elif [[ -f $SHIM_DIR/chained ]]; then
        printf '{"data":{"createCommitOnBranch":{"commit":{"oid":"oid%s"}}}}' "$n"
      else
        cat "$SHIM_DIR/graphql-response"
      fi
    fi
    ;;
  *"--jq .node_id"*) echo "repo-node-id" ;;
  *"--jq .ahead_by"*) cat "$SHIM_DIR/base-ahead" ;;
  *"contents/"*"ref=dev"*)
    [[ -f $SHIM_DIR/dev-pin ]] || exit 1
    cat "$SHIM_DIR/dev-pin"
    ;;
  *"contents/"*)
    [[ -f $SHIM_DIR/branch-pin ]] || exit 1
    cat "$SHIM_DIR/branch-pin"
    ;;
  *"state=closed"*) jq -r "${@: -1}" "$SHIM_DIR/closed-prs.json" ;;
  *"pulls?head="*) cat "$SHIM_DIR/open-pr" ;;
  *"git/ref/heads/"*)
    [[ -f $SHIM_DIR/branch-head ]] || exit 1
    cat "$SHIM_DIR/branch-head"
    ;;
  *"--jq .parents[0].sha"*) cat "$SHIM_DIR/branch-base" ;;
  *"git/commits/"*) cat "$SHIM_DIR/branch-tree" ;;
  *"--jq .files[].filename"*) cat "$SHIM_DIR/compare-files" ;;
  *"compare/"*) jq -r "${@: -1}" "$SHIM_DIR/compare.json" ;;
  *"--jq .commit.verification.verified"*) cat "$SHIM_DIR/verified" ;;
  *"--jq .html_url"*) echo "https://example.invalid/pr" ;;
esac
"#;

const BUMP_MESSAGE: &str = "feat(web): a bump\n\nWeb-Audit-Bump: part 1";

fn commit_json(parents: &[&str], committer: &str, message: &str) -> Value {
    json!({
        "parents": parents,
        "commit": {"committer": {"name": committer}, "message": message}
    })
}

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
        write(
            &site.join("tests/fixtures/web-audit-conformance/scenarios/run-a/scorecard.json"),
            "{\"score\": 1}\n",
        );
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
            (
                "compare.json",
                "{\"commits\": [{\"parents\": [\"a\"], \"commit\": {\"committer\": {\"name\": \"GitHub\"}, \"message\": \"feat(web): a bump\\n\\nWeb-Audit-Bump: part 1\"}}]}",
            ),
            ("closed-prs.json", "[]"),
            ("fetch-status", "0"),
            ("base-ahead", "0"),
            (
                "update-refs-response",
                "{\"data\":{\"updateRefs\":{\"clientMutationId\":null}}}",
            ),
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

    /// What `compare dev...<branch>` answers: `bump` commits this script made, `port` of
    /// someone else's, and `merges` GitHub made from dev.
    fn commits_past_dev(&self, bump: usize, port: usize, merges: usize) {
        let mut commits = vec![commit_json(&["a"], "GitHub", BUMP_MESSAGE); bump];
        commits.extend(vec![
            commit_json(&["a"], "A Porter", "feat(web): a port");
            port
        ]);
        commits.extend(vec![
            commit_json(&["a", "b"], "GitHub", "Merge dev");
            merges
        ]);
        self.answer("compare.json", &json!({"commits": commits}).to_string());
    }

    /// Every commit the script asked GitHub for, in order.
    fn commit_payloads(&self) -> Vec<Value> {
        (0..)
            .map(|n| self.shims.join(format!("commit-{n}.json")))
            .take_while(|path| path.exists())
            .map(|path| serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap())
            .collect()
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
    let base = run_git(&f.repo, &["rev-parse", "HEAD"]);
    assert_eq!(
        fs::read_to_string(&output_file).unwrap(),
        format!("bump=yes\nbase={base}\n")
    );

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

    f.answer("open-pr", "42");
    f.answer("branch-pin", &f.first_pin);
    let older = f.bump(&["--decide", "--on-dev-push"]);
    assert!(stdout(&older).contains("bump=yes"), "{}", stdout(&older));

    f.answer("branch-pin", &format!("{head}\n"));
    f.answer("branch-head", "bothead");
    f.answer("branch-base", "botbase");
    f.answer("compare-files", "docs/plans/a-plan.md\nCONTRIBUTING.md\n");
    let carried = f.bump(&["--decide", "--on-dev-push"]);
    assert!(stdout(&carried).contains("bump=no"), "{}", stdout(&carried));
    assert!(
        stdout(&carried).contains("already carries"),
        "{}",
        stdout(&carried)
    );
    assert!(!f.calls().contains("cargo"), "{}", f.calls());

    // A branch at the site's head with no PR open has nothing reviewing it.
    f.answer("open-pr", "");
    let no_pr = f.bump(&["--decide", "--on-dev-push"]);
    assert!(stdout(&no_pr).contains("bump=yes"), "{}", stdout(&no_pr));
    f.answer("open-pr", "42");

    for touched in [
        "src/web_audit/mod.rs",
        "tests/web_audit_cli.rs",
        "scripts/sync-web-audit.sh",
        "build_support/web_registry/mod.rs",
        "schema/scorecard.schema.json",
        "build.rs",
        "Cargo.lock",
        "rust-toolchain.toml",
        "README.md",
        ".github/workflows/web-audit-bump.yml",
    ] {
        f.answer(
            "compare-files",
            &format!("docs/plans/a-plan.md\n{touched}\n"),
        );
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
        &format!("{CORPUS}/run-b/scorecard.json"),
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
            &format!("{CORPUS}/run-a/scorecard.json"),
        ],
    );
    f.site_commit(
        &format!("{CORPUS}/run-b/scorecard.json"),
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
    for section in ["**Created:**", "**Deleted:**"] {
        assert!(
            out.contains(&format!(
                "{section} 1 vendored file, listed under Vendored files above."
            )),
            "{out}"
        );
    }
    f.assert_tree_clean();
}

#[test]
fn a_bump_that_only_changes_files_says_it_created_and_deleted_none() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    let output = f.bump(&["--dry-run"]);
    assert_ok(&output);
    let out = stdout(&output);
    for section in ["**Created:**", "**Deleted:**"] {
        assert!(out.contains(&format!("{section} None.")), "{out}");
    }
}

#[test]
fn a_line_split_by_another_tests_output_still_names_the_test_and_the_stand_down() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("suite-status", "101");
    f.answer(
        "suite-output",
        "test a_test ... standing down: other: a full run may exit 3, because of checks bound Unsupported: robots\nFAILED\n\nfailures:\n\n    a_test\n\ntest result: FAILED. 0 passed; 1 failed\n",
    );
    let output = f.bump(&["--dry-run"]);
    assert_ok(&output);
    let out = stdout(&output);
    assert!(out.contains("- `a_test`"), "{out}");
    assert!(out.contains("other: a full run may exit 3"), "{out}");
    assert!(!out.contains("did not build"), "{out}");
}

#[test]
fn the_changelog_says_when_a_bump_changes_more_than_the_pin() {
    for (file, bullet) in [
        (SITE_FILES[0], "checks, fixes and settings"),
        ("src/shared/user-agents.ts", "checks, fixes and settings"),
        (
            "tests/fixtures/web-audit-conformance/scenarios/run-c/scorecard.json",
            "reported site pin",
        ),
    ] {
        let f = Fixture::new();
        f.site_commit(file, "moved\n", "feat: a move (#2)");
        let output = f.bump(&["--dry-run"]);
        assert_ok(&output);
        let out = stdout(&output);
        let section = out.split("## Changelog").nth(1).expect("a changelog");
        assert!(section.contains(bullet), "{file}: {section}");
    }
}

#[test]
fn a_failing_suite_still_lists_the_tests_that_stood_down() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("suite-status", "101");
    f.answer(
        "suite-output",
        "standing down: a_test: a full run may exit 3, not 1, because of checks bound Unsupported: robots\ntest a_test ... FAILED\ntest result: FAILED. 0 passed; 1 failed\n",
    );
    let output = f.bump(&["--dry-run"]);
    assert_ok(&output);
    let out = stdout(&output);
    assert!(out.contains("Tests that gave up a verdict:"), "{out}");
    assert!(
        out.contains("a full run may exit 3, not 1, because of checks bound Unsupported: robots"),
        "{out}"
    );
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
        &format!("{CORPUS}/run-a/scorecard.json"),
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

#[test]
fn a_commit_with_no_pr_changing_a_standing_entry_names_that_commit_too() {
    let f = Fixture::new();
    let golden = format!("{CORPUS}/run-a/scorecard.json");
    write(
        &f.repo.join(LEDGER),
        &format!(
            "{}site_sha = \"{}\"\n\n[[divergence]]\nscenario = \"run-a\"\nelement = \"score\"\nowner = \"#120\"\n",
            empty_ledger(Path::new(env!("CARGO_MANIFEST_DIR"))),
            f.first_pin
        ),
    );
    commit(&f.repo, "feat(web): the standing entry");
    let direct = f.site_commit(&golden, "{\"score\": 2}\n", "fix: a direct push");
    f.answer("divergences.json", "[[\"run-a\", \"score\"]]");
    let output = f.bump(&["--dry-run"]);
    assert_ok(&output);
    let out = stdout(&output);
    assert!(
        out.contains(&format!(
            "site commit {} changed run-a/score and names no PR",
            &direct[..7]
        )),
        "{out}"
    );
}

#[test]
fn a_path_git_would_quote_reaches_the_commit_and_the_body_as_it_is() {
    let f = Fixture::new();
    let path = format!("{CORPUS}/café/scorecard.json");
    f.site_commit(
        &path,
        "{\"score\": 3}\n",
        "feat: a scenario with an accent (#2)",
    );
    let dry = f.bump(&["--dry-run"]);
    assert_ok(&dry);
    assert!(
        stdout(&dry).contains(&format!("- `{path}`")),
        "{}",
        stdout(&dry)
    );
    let output = f.bump(&[]);
    assert_ok(&output);
    assert_eq!(f.added(&path).as_deref(), Some("{\"score\": 3}\n"));
}

#[test]
fn a_body_past_githubs_limit_is_cut_and_keeps_its_template() {
    let f = Fixture::new();
    let long = "x".repeat(300);
    let dir = f.shims.join("body");
    fs::create_dir_all(&dir).unwrap();
    let list: String = (0..400).map(|i| format!("{long}{i}\n")).collect();
    let entries: String = (0..400)
        .map(|i| format!("[[divergence]]\nscenario = \"{long}{i}\"\nelement = \"score\"\nowner = \"#{i}\"\n\n"))
        .collect();
    write(&dir.join("changed.txt"), &list);
    write(&dir.join("created.txt"), "");
    write(&dir.join("deleted.txt"), &list);
    write(&dir.join("ledger.toml"), &entries);
    write(
        &dir.join("unported.json"),
        &json!({"handlers": (0..400).map(|i| (format!("{long}{i}"), vec!["a"])).collect::<std::collections::BTreeMap<_, _>>(), "evals": {}}).to_string(),
    );
    write(&dir.join("ledger.log"), &list);
    write(
        &dir.join("suite.log"),
        &format!("{list}test a ... FAILED\n"),
    );
    let arg = |name: &str| dir.join(name).to_str().unwrap().to_string();
    let output = Command::new("python3")
        .args(["-B", "scripts/web-audit-bump-body.py"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["--old", "0000000", "--new", "1111111"])
        .args([
            "--changed",
            &arg("changed.txt"),
            "--created",
            &arg("created.txt"),
        ])
        .args([
            "--deleted",
            &arg("deleted.txt"),
            "--ledger",
            &arg("ledger.toml"),
        ])
        .args(["--unported", &arg("unported.json")])
        .args(["--ledger-status", "1", "--ledger-log", &arg("ledger.log")])
        .args(["--suite-status", "1", "--suite-log", &arg("suite.log")])
        .output()
        .unwrap();
    assert_ok(&output);
    let out = stdout(&output);
    assert!(
        out.chars().count() <= 65_536,
        "{} characters",
        out.chars().count()
    );
    assert!(
        out.contains("Cut at GitHub's body limit"),
        "{}",
        &out[..200]
    );
    assert_eq!(
        out.matches("<details>").count(),
        out.matches("</details>").count()
    );
    assert_eq!(out.matches("```").count() % 2, 0, "a fence left open");
    for section in [
        "## Changelog",
        "## Testing",
        "## Files Modified",
        "## Deployment Notes",
    ] {
        assert!(out.contains(section), "{section}");
    }
}

#[test]
fn a_divergence_a_commit_with_no_pr_set_names_that_commit_rather_than_another_pr() {
    let f = Fixture::new();
    let golden = format!("{CORPUS}/run-a/scorecard.json");
    f.site_commit(
        &golden,
        "{\"score\": 1, \"other\": 2}\n",
        "feat: the scenario, not its score (#9001)",
    );
    let direct = f.site_commit(
        &golden,
        "{\"score\": 2, \"other\": 2}\n",
        "fix: a direct push",
    );
    f.answer("divergences.json", "[[\"run-a\", \"score\"]]");
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
    assert!(
        out.contains(&format!(
            "site commit {} changed run-a/score and names no PR",
            &direct[..7]
        )),
        "{out}"
    );
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
            &format!("{CORPUS}/run-a/scorecard.json"),
        ],
    );
    let new = f.site_commit(
        &format!("{CORPUS}/run-b/scorecard.json"),
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
        &format!("{CORPUS}/run-a/scorecard.json"),
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
    let moved: Value =
        serde_json::from_str(&fs::read_to_string(f.shims.join("update-refs.json")).unwrap())
            .unwrap();
    assert_eq!(moved["variables"]["input"]["repositoryId"], "repo-node-id");
    assert_eq!(
        moved["variables"]["input"]["refUpdates"],
        json!([{
            "name": "refs/heads/bot/web-audit-bump",
            "beforeOid": "oldhead",
            "afterOid": "newoid",
            "force": true
        }])
    );
    let calls = f.calls();
    assert!(
        !calls.contains("git/refs/heads/bot/web-audit-bump -f sha"),
        "a REST update cannot say what it expects to replace: {calls}"
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
    f.commits_past_dev(1, 1, 0);
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
fn port_commits_on_a_branch_with_no_open_pr_also_stand_down() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("branch-head", "porthead");
    f.commits_past_dev(1, 1, 0);
    f.answer(
        "closed-prs.json",
        &json!([{"merged_at": null, "head": {"sha": "porthead"}}]).to_string(),
    );
    let output = f.bump(&[]);
    assert_ok(&output);
    assert!(
        stdout(&output).contains("standing down"),
        "{}",
        stdout(&output)
    );
    let calls = f.calls();
    assert!(!calls.contains("graphql"), "{calls}");
    assert!(!calls.contains("-X POST"), "{calls}");
}

#[test]
fn a_branch_a_merged_pr_left_behind_is_not_port_work() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("branch-head", "porthead");
    f.answer("branch-tree", "oldtree");
    f.commits_past_dev(1, 2, 0);
    f.answer(
        "closed-prs.json",
        &json!([{"merged_at": "2026-11-04T00:00:00Z", "head": {"sha": "porthead"}}]).to_string(),
    );
    let output = f.bump(&[]);
    assert_ok(&output);
    assert!(
        !stdout(&output).contains("standing down"),
        "{}",
        stdout(&output)
    );
    assert!(
        f.calls().contains("api -X POST repos/owner/repo/pulls"),
        "{}",
        f.calls()
    );
}

#[test]
fn a_merge_from_dev_is_not_port_work() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("open-pr", "42");
    f.answer("branch-head", "oldhead");
    f.answer("branch-tree", "oldtree");
    f.commits_past_dev(1, 0, 1);
    let output = f.bump(&[]);
    assert_ok(&output);
    assert!(
        !stdout(&output).contains("standing down"),
        "{}",
        stdout(&output)
    );
    assert!(f.shims.join("update-refs.json").exists(), "{}", f.calls());
}

#[test]
fn a_bump_of_several_commits_is_not_port_work() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("open-pr", "42");
    f.answer("branch-head", "oldhead");
    f.answer("branch-tree", "oldtree");
    f.commits_past_dev(5, 0, 0);
    let output = f.bump(&[]);
    assert_ok(&output);
    assert!(
        !stdout(&output).contains("standing down"),
        "{}",
        stdout(&output)
    );
    assert!(f.shims.join("update-refs.json").exists(), "{}", f.calls());
}

/// A commit GitHub cannot finish in about ten seconds answers 504 and does not land.
#[test]
fn a_bump_too_large_for_one_commit_lands_as_a_chain_of_verified_commits() {
    const FILES: usize = 230;
    let f = Fixture::new();
    let base = run_git(&f.repo, &["rev-parse", "HEAD"]);
    for n in 0..FILES {
        write(
            &f.site.join(format!("{CORPUS}/many-{n:03}/scorecard.json")),
            "{\"score\": 1}\n",
        );
    }
    commit(&f.site, "feat: every golden moves (#2)");
    assert_ok(&f.bump(&[]));

    let commits = f.commit_payloads();
    assert_eq!(commits.len(), 3, "{} commits", commits.len());
    let mut landed = 0;
    for (n, payload) in commits.iter().enumerate() {
        let input = &payload["variables"]["input"];
        let expected = if n == 0 { base.as_str() } else { "newoid" };
        assert_eq!(input["expectedHeadOid"], expected, "commit {n}");
        assert_eq!(
            input["message"]["body"],
            format!("Web-Audit-Bump: part {}", n + 1)
        );
        let additions = input["fileChanges"]["additions"].as_array().unwrap();
        assert!(additions.len() <= 100, "commit {n}: {}", additions.len());
        landed += additions.len();
    }
    assert_eq!(landed, FILES + 1, "every golden and the pin");
    let calls = f.calls();
    assert_eq!(
        calls.matches("--jq .commit.verification.verified").count(),
        3,
        "{calls}"
    );
    assert_eq!(
        calls
            .matches("-f ref=refs/heads/bot/web-audit-bump-next")
            .count(),
        1,
        "{calls}"
    );
    assert_eq!(
        calls.matches("api -X POST repos/owner/repo/pulls").count(),
        1,
        "{calls}"
    );
    assert!(
        calls.contains("-f ref=refs/heads/bot/web-audit-bump -f sha=newoid"),
        "{calls}"
    );
}

#[test]
fn a_bump_of_large_files_splits_by_size_too() {
    let f = Fixture::new();
    let body = format!("{{\"pad\": \"{}\"}}\n", "x".repeat(1_600_000));
    for n in 0..3 {
        write(
            &f.site.join(format!("{CORPUS}/big-{n}/scorecard.json")),
            &body,
        );
    }
    commit(&f.site, "feat: large goldens (#2)");
    assert_ok(&f.bump(&[]));
    let commits = f.commit_payloads();
    assert!(commits.len() >= 3, "{} commits", commits.len());
    for payload in &commits {
        let size: usize = payload["variables"]["input"]["fileChanges"]["additions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["contents"].as_str().unwrap().len())
            .sum();
        assert!(size <= 3 * 1024 * 1024, "{size} bytes in one commit");
    }
}

fn a_run_with_these_commits_past_dev(commits: &[Value]) -> Fixture {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("open-pr", "42");
    f.answer("branch-head", "oldhead");
    f.answer("branch-tree", "oldtree");
    f.answer("compare.json", &json!({ "commits": commits }).to_string());
    f
}

#[test]
fn an_amended_bump_commit_keeps_the_mark_and_still_reads_as_port_work() {
    let f = a_run_with_these_commits_past_dev(&[commit_json(&["a"], "A Porter", BUMP_MESSAGE)]);
    let output = f.bump(&[]);
    assert_ok(&output);
    assert!(
        stdout(&output).contains("standing down"),
        "{}",
        stdout(&output)
    );
    assert!(!f.shims.join("update-refs.json").exists(), "{}", f.calls());
}

#[test]
fn a_hand_resolved_merge_reads_as_port_work_and_a_merge_github_made_does_not() {
    let hand = a_run_with_these_commits_past_dev(&[
        commit_json(&["a"], "GitHub", BUMP_MESSAGE),
        commit_json(&["a", "b"], "A Porter", "Merge dev"),
    ]);
    let output = hand.bump(&[]);
    assert_ok(&output);
    assert!(
        stdout(&output).contains("standing down"),
        "{}",
        stdout(&output)
    );

    let github = a_run_with_these_commits_past_dev(&[
        commit_json(&["a"], "GitHub", BUMP_MESSAGE),
        commit_json(&["a", "b"], "GitHub", "Merge dev"),
    ]);
    let output = github.bump(&[]);
    assert_ok(&output);
    assert!(
        !stdout(&output).contains("standing down"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn a_commit_github_made_with_the_mark_only_in_its_headline_reads_as_port_work() {
    let f = a_run_with_these_commits_past_dev(&[commit_json(
        &["a"],
        "GitHub",
        "Web-Audit-Bump: part 1 of the port",
    )]);
    let output = f.bump(&[]);
    assert_ok(&output);
    assert!(
        stdout(&output).contains("standing down"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn a_base_with_commits_dev_lacks_publishes_nothing() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("base-ahead", "2");
    let output = f.bump(&[]);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("commits dev lacks"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!f.calls().contains("graphql"), "{}", f.calls());
}

#[test]
fn a_failed_download_publishes_nothing_and_runs_no_suite() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("fetch-status", "101");
    let output = f.bump(&[]);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cargo fetch failed"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = f.calls();
    assert!(!calls.contains("cargo test"), "{calls}");
    assert!(!calls.contains("gh "), "{calls}");
}

/// Re-pins the repository to site dev's head, as a merged bump does.
fn land_site_head(f: &Fixture) {
    let head = run_git(&f.site, &["rev-parse", "dev"]);
    let mut sync = Command::new("bash");
    sync.current_dir(&f.repo)
        .args(["scripts/sync-web-audit.sh", "--pin", &head])
        .env("WEB_AUDIT_SITE_REPO", &f.site);
    scrub_repo_env(&mut sync);
    assert!(sync.output().unwrap().status.success());
    commit(&f.repo, "feat(web): a merged bump");
}

fn changes_in(payload: &Value, kind: &str) -> Vec<String> {
    payload["variables"]["input"]["fileChanges"][kind]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| change["path"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn a_chain_carries_every_change_once_with_each_commit_on_the_last() {
    let f = Fixture::new();
    let (old, new) = (120, 130);
    for n in 0..old {
        write(
            &f.site.join(format!("{CORPUS}/old-{n:03}/scorecard.json")),
            "{\"score\": 1}\n",
        );
    }
    commit(&f.site, "feat: goldens to remove later (#2)");
    land_site_head(&f);
    let base = run_git(&f.repo, &["rev-parse", "HEAD"]);
    run_git(
        &f.site,
        &["rm", "-r", "--quiet", "--", &format!("{CORPUS}/old-*")],
    );
    for n in 0..new {
        write(
            &f.site.join(format!("{CORPUS}/new-{n:03}/scorecard.json")),
            "{\"score\": 2}\n",
        );
    }
    commit(&f.site, "feat: the goldens move (#3)");
    f.answer("chained", "");
    assert_ok(&f.bump(&[]));

    let commits = f.commit_payloads();
    assert_eq!(commits.len(), 3, "{} commits", commits.len());
    let mut added = BTreeSet::new();
    let mut deleted = BTreeSet::new();
    for (n, payload) in commits.iter().enumerate() {
        let expected = if n == 0 {
            base.clone()
        } else {
            format!("oid{}", n - 1)
        };
        assert_eq!(
            payload["variables"]["input"]["expectedHeadOid"], expected,
            "commit {n}"
        );
        let (a, d) = (
            changes_in(payload, "additions"),
            changes_in(payload, "deletions"),
        );
        assert!(!a.is_empty() || !d.is_empty(), "commit {n} is empty");
        assert!(
            a.len() + d.len() <= 100,
            "commit {n}: {}",
            a.len() + d.len()
        );
        added.extend(a);
        deleted.extend(d);
    }
    assert_eq!(added.len(), new + 1, "the new goldens and the pin");
    assert_eq!(deleted.len(), old, "the removed goldens");
    assert!(
        f.calls()
            .contains("-f ref=refs/heads/bot/web-audit-bump -f sha=oid2"),
        "the branch lands on the last commit: {}",
        f.calls()
    );
}

#[test]
fn a_failure_partway_through_a_chain_moves_no_branch_and_cleans_up() {
    let f = Fixture::new();
    for n in 0..150 {
        write(
            &f.site.join(format!("{CORPUS}/many-{n:03}/scorecard.json")),
            "{\"score\": 1}\n",
        );
    }
    commit(&f.site, "feat: many goldens (#2)");
    f.answer("chained", "");
    f.answer("fail-at-commit", "1");
    let output = f.bump(&[]);
    assert_eq!(output.status.code(), Some(1), "{}", stdout(&output));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("commit 1 failed"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = f.calls();
    assert!(
        !calls.contains("heads/bot/web-audit-bump -f sha"),
        "{calls}"
    );
    assert!(!calls.contains("repos/owner/repo/pulls -f"), "{calls}");
    assert!(
        calls.ends_with("api -X DELETE repos/owner/repo/git/refs/heads/bot/web-audit-bump-next\n"),
        "{calls}"
    );
}

#[test]
fn a_single_file_over_the_byte_limit_goes_alone_with_no_empty_commit_first() {
    let f = Fixture::new();
    let body = format!("{{\"pad\": \"{}\"}}\n", "x".repeat(3_500_000));
    write(&f.site.join(format!("{CORPUS}/huge/scorecard.json")), &body);
    commit(&f.site, "feat: one huge golden (#2)");
    assert_ok(&f.bump(&[]));
    for (n, payload) in f.commit_payloads().iter().enumerate() {
        assert!(
            !changes_in(payload, "additions").is_empty(),
            "commit {n} is empty"
        );
    }
}

#[test]
fn a_port_commit_pushed_during_the_run_stops_the_forced_update() {
    let f = Fixture::new();
    f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("open-pr", "42");
    f.answer("branch-head", "oldhead");
    f.answer("branch-tree", "oldtree");
    f.answer(
        "update-refs-response",
        "{\"errors\": [{\"message\": \"the ref moved\"}]}",
    );
    let output = f.bump(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("the ref moved"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = f.calls();
    assert!(!calls.contains("repos/owner/repo/pulls/42"), "{calls}");
    assert!(
        calls.ends_with("api -X DELETE repos/owner/repo/git/refs/heads/bot/web-audit-bump-next\n"),
        "{calls}"
    );
}

#[test]
fn a_merge_during_the_run_publishes_nothing() {
    let f = Fixture::new();
    let new = f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
    f.answer("dev-pin", &format!("{new}\n"));
    let output = f.bump(&[]);
    assert_ok(&output);
    assert!(
        stdout(&output).contains("already holds pin"),
        "{}",
        stdout(&output)
    );
    let calls = f.calls();
    assert!(!calls.contains("graphql"), "{calls}");
    assert!(!calls.contains("-X POST"), "{calls}");
    assert!(!calls.contains("-X PATCH"), "{calls}");
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
    assert_eq!(fs::read_to_string(&output_file).unwrap(), "ready=yes\n");
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
fn publish_refuses_a_symlink_anywhere_in_the_prepared_directory() {
    for link in [
        "title".to_string(),
        "body.md".to_string(),
        format!("tree/{REGISTRY}"),
    ] {
        let f = Fixture::new();
        f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
        let prepared = f.prepare();
        let path = prepared.join(&link);
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink("/proc/self/environ", &path).unwrap();
        let checkout = f.second_job_checkout();
        let output = f.publish_in(&checkout, &prepared);
        assert_eq!(output.status.code(), Some(1), "{link}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("symlink"),
            "{link}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!f.calls().contains("gh "), "{link}: {}", f.calls());
    }
}

#[test]
fn publish_refuses_a_prepared_directory_missing_a_part_and_deletes_nothing() {
    for missing in [
        "tree".to_string(),
        format!("tree/{PIN_FILE}"),
        "title".to_string(),
        "body.md".to_string(),
    ] {
        let f = Fixture::new();
        f.site_commit(SITE_FILES[0], "moved\n", "feat: a registry move (#2)");
        let prepared = f.prepare();
        let path = prepared.join(&missing);
        if path.is_dir() {
            fs::remove_dir_all(&path).unwrap();
        } else {
            fs::remove_file(&path).unwrap();
        }
        let checkout = f.second_job_checkout();
        let output = f.publish_in(&checkout, &prepared);
        assert_eq!(output.status.code(), Some(1), "{missing}");
        assert!(!f.calls().contains("gh "), "{missing}: {}", f.calls());
        assert!(checkout.join(REGISTRY).exists(), "{missing}");
    }
}

#[test]
fn a_symlink_in_a_site_commit_stops_the_bump_before_anything_is_prepared() {
    let f = Fixture::new();
    std::os::unix::fs::symlink(
        "/proc/self/environ",
        f.site.join(CORPUS).join("run-a/leak.json"),
    )
    .unwrap();
    commit(&f.site, "feat: a scenario that links out (#2)");
    let prepared = f.shims.join("prepared");
    let output = f.bump(&["--prepare", prepared.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("symlink") && stderr.contains("leak.json"),
        "{stderr}"
    );
    assert!(!prepared.exists(), "nothing reached upload-artifact");
    assert!(!f.calls().contains("cargo"), "{}", f.calls());
}

#[test]
fn the_token_job_checks_out_a_commit_named_before_any_test_code_ran() {
    let workflow = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/web-audit-bump.yml"),
    )
    .unwrap();
    let (first_job, second_job) = workflow.split_once("\n  publish:\n").unwrap();
    assert!(
        first_job.contains("base: ${{ steps.decide.outputs.base }}"),
        "{first_job}"
    );
    assert!(!first_job.contains("secrets."), "{first_job}");
    let (_, decide_onward) = first_job.split_once("- name: Decide\n").unwrap();
    let (decide, test_code) = decide_onward.split_once("- name: Install Rust\n").unwrap();
    assert!(decide.contains("GH_TOKEN: ${{ github.token }}"), "{decide}");
    assert!(
        !test_code.contains("GH_TOKEN")
            && !first_job
                .split("- name: Decide\n")
                .next()
                .unwrap()
                .contains("GH_TOKEN"),
        "only the Decide step holds the read token: {first_job}"
    );
    for job in [first_job, second_job] {
        assert!(job.contains("persist-credentials: false"), "{job}");
    }
    assert!(
        second_job.contains("ref: ${{ needs.prepare.outputs.base }}"),
        "{second_job}"
    );
    assert!(
        second_job.contains("secrets.CI_RELEASE_TOKEN"),
        "{second_job}"
    );
}

#[test]
fn two_bumps_in_a_row_carry_an_entry_no_commit_in_the_second_range_touched() {
    let f = Fixture::new();
    f.site_commit(
        &format!("{CORPUS}/run-a/scorecard.json"),
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

    fn advance_site_dev(&self, file: &str) {
        write(&self.site.join(file), "moved\n");
        commit(&self.site, "feat: site dev moves");
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
    assert!(
        out.contains("pass\tthe pin carries site dev's vendored inputs"),
        "{out}"
    );
}

#[test]
fn the_gate_refuses_a_pin_behind_a_dev_commit_that_moves_a_vendored_input() {
    let tree = GateTree::new(Some("# header only\n"), true, Some(ALL_PORTED));
    tree.advance_site_dev(SITE_FILES[0]);
    let (out, code) = tree.gate();
    assert_eq!(code, Some(1), "{out}");
    assert!(
        out.contains("fail\tweb-audit-stale: site dev has moved vendored inputs past the pin"),
        "{out}"
    );
    assert!(out.contains(SITE_FILES[0]), "{out}");
    assert!(out.contains("head%3Abot%2Fweb-audit-bump"), "{out}");
    assert!(
        out.contains("gh workflow run web-audit-bump.yml --ref dev"),
        "{out}"
    );
    assert!(
        out.contains("Fix: ") && out.contains(HANDLING_DOCS),
        "{out}"
    );
}

#[test]
fn the_gate_passes_a_pin_behind_a_dev_commit_that_moves_nothing_vendored() {
    let tree = GateTree::new(Some("# header only\n"), true, Some(ALL_PORTED));
    tree.advance_site_dev("NEWS.md");
    let (out, code) = tree.gate();
    assert_eq!(code, Some(0), "{out}");
    assert!(
        out.contains("pass\tthe pin carries site dev's vendored inputs"),
        "{out}"
    );
}

#[test]
fn the_gate_reads_a_pin_with_no_trailing_newline() {
    let tree = GateTree::new(Some("# header only\n"), true, Some(ALL_PORTED));
    let pin_file = tree.tmp.path().join(PIN_FILE);
    let pin = fs::read_to_string(&pin_file).unwrap();
    fs::write(&pin_file, pin.trim_end()).unwrap();
    let (out, code) = tree.gate();
    assert_eq!(code, Some(0), "{out}");
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

#[test]
fn smoke_fails_when_the_gate_dies_or_says_nothing() {
    for gate in [
        "#!/usr/bin/env bash\nexit 7\n",
        "#!/usr/bin/env bash\nprintf 'pass\\tthe divergence ledger has no entries\\t\\n'\nexit 7\n",
        "#!/usr/bin/env bash\nexit 0\n",
    ] {
        let tree = GateTree::new(Some("# header only\n"), true, Some(ALL_PORTED));
        fs::write(
            tree.tmp.path().join("scripts/release/web-parity-gate.sh"),
            gate,
        )
        .unwrap();
        let out = tree.smoke();
        assert!(
            out.contains("✗ web-audit-gate: the parity gate did not finish"),
            "{gate}: {out}"
        );
    }
}

#[test]
fn smoke_skips_a_stripped_tree_that_no_longer_carries_the_gate() {
    let tree = GateTree::new(None, false, None);
    fs::remove_file(tree.tmp.path().join("scripts/release/web-parity-gate.sh")).unwrap();
    let out = tree.smoke();
    assert!(out.contains("⊝ web audit parity"), "{out}");
    assert!(!out.contains("web-audit-gate"), "{out}");
}

#[test]
fn the_gate_workflow_runs_the_gate_only_on_a_tree_that_ships_the_audit() {
    let workflow = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(".github/workflows/web-audit-release-gate.yml"),
    )
    .unwrap();
    assert!(
        workflow.contains("- name: Gate\n        if: steps.tree.outputs.ships == 'yes'\n"),
        "{workflow}"
    );
}

// The first-run smoke over a stand-in binary. openssl and timeout are the script's own
// requirements, and only the Linux job runs it.

#[cfg(target_os = "linux")]
fn first_run_smoke(checks: &Value, scorecard: &Value, code: i32) -> Output {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    fs::write(dir.join("checks.json"), checks.to_string()).unwrap();
    fs::write(dir.join("scorecard.json"), scorecard.to_string()).unwrap();
    let anc = dir.join("anc");
    fs::write(
        &anc,
        format!(
            "#!/usr/bin/env bash\nif [[ $1 == emit ]]; then cat '{0}/checks.json'; exit 0; fi\ncat '{0}/scorecard.json'\nexit {code}\n",
            dir.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&anc, fs::Permissions::from_mode(0o755)).unwrap();
    // jq reads every filter the script uses, and CI installs jaq only for that job.
    let jaq = dir.join("jaq");
    fs::write(&jaq, "#!/usr/bin/env bash\nexec jq \"$@\"\n").unwrap();
    fs::set_permissions(&jaq, fs::Permissions::from_mode(0o755)).unwrap();
    Command::new("bash")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("scripts/smoke-web-audit.sh")
        .arg(&anc)
        .env(
            "PATH",
            format!(
                "{}:{}",
                dir.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .unwrap()
}

#[cfg(target_os = "linux")]
#[test]
fn the_first_run_smoke_takes_exit_3_only_from_a_check_this_build_cannot_run() {
    let ported = json!({"checks": [{"id": "a", "ported": true}, {"id": "b", "ported": true}]});
    let unported = json!({"checks": [{"id": "a", "ported": false}, {"id": "b", "ported": true}]});
    let rows = |status: &str| {
        json!({
            "summary": {"pass": 1},
            "results": [{"id": "a", "status": "skip"}, {"id": "b", "status": status}]
        })
    };

    let no_skips = json!({
        "summary": {"pass": 1},
        "results": [{"id": "a", "status": "n_a"}, {"id": "b", "status": "pass"}]
    });

    let stood_down = first_run_smoke(&unported, &rows("pass"), 3);
    let stderr = String::from_utf8_lossy(&stood_down.stderr);
    assert_eq!(stood_down.status.code(), Some(0), "{stderr}");
    assert!(
        stderr.contains("standing down") && stderr.contains(": a"),
        "{stderr}"
    );

    for (what, checks, scorecard, code) in [
        ("nothing is unported", &ported, rows("pass"), 3),
        ("a row errored", &unported, rows("error"), 3),
        ("a row the deadline cut short", &unported, rows("skip"), 3),
        ("no row explains the 3", &unported, no_skips.clone(), 3),
        ("a later exit code", &unported, rows("pass"), 4),
    ] {
        let output = first_run_smoke(checks, &scorecard, code);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{what}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    assert_eq!(
        first_run_smoke(&ported, &rows("pass"), 1).status.code(),
        Some(0)
    );
}
