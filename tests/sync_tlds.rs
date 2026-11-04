//! `scripts/sync-tlds.sh` replaces the vendored list only with a download that
//! reads as the list, because bare routing trusts that file. A `curl` shim serves
//! each download and a `git` shim points the script at a scratch tree, so the
//! test needs no network and leaves the real file alone.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

const DEST: &str = "src/web_audit/tlds-alpha-by-domain.txt";
const SENTINEL: &str = "the list as it was\n";
const STAMP: &str = "# Version 2026110300, Last Updated Tue Nov  3 07:07:01 2026 UTC\n";

fn write_shim(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Runs the script against `download`, and returns what it did and the file it
/// was asked to replace.
fn refresh_with(download: &str) -> (Output, String) {
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
        &format!("#!/usr/bin/env bash\nprintf '%s\\n' {}\n", tree.display()),
    );

    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/sync-tlds.sh");
    let output = Command::new("bash")
        .arg(script)
        .env("PATH", path)
        .output()
        .expect("run sync-tlds.sh");
    let kept = fs::read_to_string(&dest).unwrap();
    (output, kept)
}

fn names(count: usize) -> String {
    (0..count).map(|i| format!("TLD{i}\n")).collect()
}

#[test]
fn a_page_with_no_version_stamp_leaves_the_list_as_it_was() {
    let (output, kept) = refresh_with(&format!("<html>captive portal</html>\n{}", names(1100)));
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(kept, SENTINEL);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no version stamp"), "{stderr}");
}

#[test]
fn a_line_that_is_not_a_domain_leaves_the_list_as_it_was() {
    let (output, kept) = refresh_with(&format!("{STAMP}{}<a href>\n", names(1100)));
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(kept, SENTINEL);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not a top-level domain"), "{stderr}");
}

#[test]
fn a_bad_early_line_in_a_large_download_leaves_the_list_as_it_was() {
    let (output, kept) = refresh_with(&format!("{STAMP}<a href>\n{}", names(40_000)));
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(kept, SENTINEL);
}

#[test]
fn a_lowercase_name_leaves_the_list_as_it_was() {
    let (output, kept) = refresh_with(&format!("{STAMP}{}com\n", names(1100)));
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(kept, SENTINEL);
}

#[test]
fn a_truncated_list_leaves_the_list_as_it_was() {
    let (output, kept) = refresh_with(&format!("{STAMP}{}", names(999)));
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(kept, SENTINEL);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("fewer than 1000"), "{stderr}");
}

#[test]
fn a_list_that_reads_as_the_list_replaces_it() {
    let download = format!("{STAMP}{}", names(1000));
    let (output, kept) = refresh_with(&download);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(kept, download);
}
