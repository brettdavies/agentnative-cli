//! `scripts/sync-linguist-extensions.sh` replaces the vendored list only with a
//! download that reads as Linguist's, because bare routing trusts that file. A
//! `curl` shim serves each download and a `git` shim names a commit and points the
//! script at a scratch tree, so the test needs no network and leaves the real file
//! alone.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

const DEST: &str = "src/web_audit/linguist-extensions.txt";
const SENTINEL: &str = "the list as it was\n";
const SHA: &str = "c26d14a50f639607c5c80e99187e5ca39fc1695e";

fn write_shim(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Runs the script against `download` with `remote` as what `git ls-remote` prints,
/// and returns what it did and the file it was asked to replace.
fn refresh_with(download: &str, remote: &str) -> (Output, String) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let tree = tmp.path().join("tree");
    let dest = tree.join(DEST);
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    fs::write(&dest, SENTINEL).unwrap();
    let payload = tmp.path().join("payload");
    fs::write(&payload, download).unwrap();

    let bin = tmp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    write_shim(
        &bin.join("curl"),
        &format!(
            "#!/usr/bin/env bash\n\
             while [ $# -gt 0 ]; do\n\
               if [ \"$1\" = -o ]; then cp {} \"$2\"; exit 0; fi\n\
               shift\n\
             done\n\
             exit 1\n",
            payload.display()
        ),
    );
    write_shim(
        &bin.join("git"),
        &format!(
            "#!/usr/bin/env bash\n\
             case \"$1\" in\n\
               rev-parse) printf '%s\\n' {} ;;\n\
               ls-remote) printf '%s\\n' '{remote}' ;;\n\
             esac\n",
            tree.display()
        ),
    );

    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/sync-linguist-extensions.sh");
    let output = Command::new("bash")
        .arg(script)
        .env("PATH", path)
        .output()
        .expect("run sync-linguist-extensions.sh");
    let kept = fs::read_to_string(&dest).unwrap();
    (output, kept)
}

fn remote() -> String {
    format!("{SHA}\tHEAD")
}

/// A languages.yml holding `count` extensions, shaped as Linguist's is.
fn languages(count: usize) -> String {
    let extensions: String = (0..count).map(|i| format!("  - \".ext{i}\"\n")).collect();
    format!(
        "ABAP:\n  type: programming\n  extensions:\n{extensions}  interpreters:\n  - notanextension\n"
    )
}

fn assert_refused(output: &Output, kept: &str, why: &str) {
    assert!(!output.status.success(), "{why}: {output:?}");
    assert_eq!(kept, SENTINEL, "{why}: the file changed");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("left as it was"), "{why}: {stderr}");
}

#[test]
fn a_good_download_replaces_the_list_with_each_extensions_last_label() {
    let download = format!(
        "{}Ruby:\n  extensions:\n  - \".html.erb\"\n  - \".RS\"\n  - \".c++\"\n  - .bare\n  filenames:\n  - Gemfile.lock\n",
        languages(1000)
    );
    let (output, kept) = refresh_with(&download, &remote());
    assert!(output.status.success(), "{output:?}");
    let mut lines = kept.lines();
    assert_eq!(
        lines.next(),
        Some(format!("# github-linguist/linguist@{SHA} lib/linguist/languages.yml (MIT)").as_str())
    );
    let names: Vec<&str> = lines.collect();
    assert!(names.contains(&"erb"), "the last label of a compound");
    assert!(names.contains(&"rs"), "lowercased");
    assert!(names.contains(&"bare"), "an unquoted item");
    assert!(!names.contains(&"c++"), "cannot be a label");
    assert!(!names.contains(&"lock"), "a filename is no extension");
    assert!(!names.contains(&"notanextension"), "another key's list");
    assert!(names.is_sorted_by(|a, b| a < b), "sorted and unique");
}

#[test]
fn a_captive_portal_page_leaves_the_list_as_it_was() {
    let (output, kept) = refresh_with("<html><body>sign in</body></html>\n", &remote());
    assert_refused(&output, &kept, "a captive portal");
}

#[test]
fn a_truncated_download_leaves_the_list_as_it_was() {
    let (output, kept) = refresh_with(&languages(999), &remote());
    assert_refused(&output, &kept, "too few extensions");
}

#[test]
fn a_remote_that_names_no_commit_leaves_the_list_as_it_was() {
    let (output, kept) = refresh_with(&languages(1000), "");
    assert_refused(&output, &kept, "no commit");
}
