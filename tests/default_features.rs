//! What the default feature set holds. A release ships the default build, so
//! this is what users get: these tests run in the required CI jobs, which the
//! release binary size job and its first-run smoke are not.

use assert_cmd::Command;

fn anc() -> Command {
    Command::cargo_bin("anc").expect("the anc binary")
}

fn help() -> String {
    let output = anc().arg("--help").assert().success();
    String::from_utf8_lossy(&output.get_output().stdout).into_owned()
}

fn lists_web_verb(help: &str) -> bool {
    help.lines()
        .any(|line| line.trim_start().starts_with("web "))
}

// Reads the manifest rather than the compiled features, so it holds in the
// `--no-default-features` job too.
#[test]
fn the_default_build_carries_the_web_audit() {
    let manifest: toml::Table = include_str!("../Cargo.toml").parse().expect("Cargo.toml");
    let default = manifest["features"]["default"]
        .as_array()
        .expect("a default feature list");
    assert!(
        default
            .iter()
            .any(|feature| feature.as_str() == Some("web")),
        "`web` left the default features, so releases stop shipping `anc web`: {default:?}"
    );
}

#[cfg(feature = "web")]
#[test]
fn a_build_with_the_feature_lists_the_web_verb() {
    assert!(lists_web_verb(&help()));
}

#[cfg(not(feature = "web"))]
mod without_the_feature {
    use super::{anc, help, lists_web_verb};

    #[test]
    fn help_lists_no_web_verb() {
        assert!(!lists_web_verb(&help()));
    }

    #[test]
    fn the_web_readers_are_absent() {
        for reader in ["web-checks", "web-schema", "web-remediation"] {
            anc().args(["emit", reader]).assert().code(2);
        }
    }

    #[test]
    fn a_network_shaped_argument_routes_to_the_offline_audit() {
        let output = anc().arg("anc-web-audit-missing.dev").assert().code(2);
        let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
        assert!(stderr.contains("path does not exist"), "{stderr}");
    }
}
