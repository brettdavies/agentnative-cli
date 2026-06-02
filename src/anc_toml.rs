//! `.anc.toml` loader — per-CLI configuration discovered at the audit
//! target's repo root.
//!
//! Today the schema carries one section:
//!
//! ```toml
//! [p6]
//! domain_verbs = ["mentions", "timeline", "whoami"]
//! ```
//!
//! `domain_verbs` extends the built-in standard-verb list consulted by the
//! `p6-may-standard-names` audit. Built-ins stay conservative across all
//! CLIs; CLIs whose platform vocabulary diverges from the global verb set
//! (e.g. an X CLI shipping `post` / `like` / `repost`) declare those verbs
//! here instead of being penalized for using their native terminology.
//!
//! Loader contract:
//!
//! - Missing `.anc.toml` returns [`AncConfigLoad::Absent`] — the loader is
//!   additive, never required.
//! - A parse error returns [`AncConfigLoad::Invalid`] carrying the formatted
//!   diagnostic so audits can surface it in their evidence string.
//! - A path that isn't a directory (binary-mode audit targets, or pathological
//!   paths) returns [`Absent`][AncConfigLoad::Absent].

use std::fs;
use std::path::Path;

use serde::Deserialize;

/// Filename probed at the audit target root.
pub const ANC_TOML_FILENAME: &str = ".anc.toml";

/// Root document for `.anc.toml`. New sections land here as the schema grows.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
pub struct AncConfig {
    #[serde(default)]
    pub p6: P6Config,
}

/// `[p6]` section — per-principle config bag for P6 (Predictable Surface).
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
pub struct P6Config {
    /// Per-CLI domain vocabulary that augments the global standard-verb list.
    /// Treated additively: a verb is recognized if it appears in the built-in
    /// list OR this slice.
    #[serde(default)]
    pub domain_verbs: Vec<String>,
}

/// Outcome of probing a target directory for `.anc.toml`. `Absent` is the
/// happy path for the overwhelming majority of CLIs; `Loaded` carries the
/// parsed config; `Invalid` carries a human-readable parse error suitable
/// for surfacing in audit evidence (audits should generally render as
/// `Warn`, not silently swallow).
#[derive(Debug, PartialEq, Eq)]
pub enum AncConfigLoad {
    Absent,
    Loaded(AncConfig),
    Invalid(String),
}

impl AncConfigLoad {
    /// Borrow the loaded config, if any. Useful when callers just want the
    /// `domain_verbs` slice and treat `Absent`/`Invalid` identically.
    pub fn as_config(&self) -> Option<&AncConfig> {
        match self {
            AncConfigLoad::Loaded(cfg) => Some(cfg),
            _ => None,
        }
    }
}

/// Probe `repo_root/.anc.toml`. `repo_root` may be a directory or a file —
/// binary-mode audit targets pass a file path, in which case `.anc.toml`
/// doesn't apply and the loader returns `Absent`.
pub fn load(repo_root: &Path) -> AncConfigLoad {
    if !repo_root.is_dir() {
        return AncConfigLoad::Absent;
    }
    let candidate = repo_root.join(ANC_TOML_FILENAME);
    let raw = match fs::read_to_string(&candidate) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return AncConfigLoad::Absent,
        Err(e) => return AncConfigLoad::Invalid(format!("could not parse .anc.toml: {e}")),
    };
    match toml::from_str::<AncConfig>(&raw) {
        Ok(cfg) => AncConfigLoad::Loaded(cfg),
        Err(e) => AncConfigLoad::Invalid(format!("could not parse .anc.toml: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_tmp(label: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "anc-toml-{label}-{}-{id}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("after epoch")
                .as_nanos(),
        ));
        fs::create_dir_all(&dir).expect("create tempdir");
        dir
    }

    #[test]
    fn absent_when_no_file() {
        let dir = unique_tmp("absent");
        assert_eq!(load(&dir), AncConfigLoad::Absent);
    }

    #[test]
    fn loaded_with_domain_verbs() {
        let dir = unique_tmp("loaded");
        fs::write(
            dir.join(ANC_TOML_FILENAME),
            "[p6]\ndomain_verbs = [\"post\", \"like\"]\n",
        )
        .expect("write .anc.toml");
        match load(&dir) {
            AncConfigLoad::Loaded(cfg) => {
                assert_eq!(cfg.p6.domain_verbs, vec!["post", "like"]);
            }
            other => panic!("expected Loaded, got {other:?}"),
        }
    }

    #[test]
    fn loaded_with_empty_domain_verbs() {
        let dir = unique_tmp("empty");
        fs::write(dir.join(ANC_TOML_FILENAME), "[p6]\ndomain_verbs = []\n")
            .expect("write .anc.toml");
        match load(&dir) {
            AncConfigLoad::Loaded(cfg) => assert!(cfg.p6.domain_verbs.is_empty()),
            other => panic!("expected Loaded, got {other:?}"),
        }
    }

    #[test]
    fn loaded_without_p6_section() {
        let dir = unique_tmp("no-p6");
        fs::write(dir.join(ANC_TOML_FILENAME), "# empty config\n").expect("write .anc.toml");
        match load(&dir) {
            AncConfigLoad::Loaded(cfg) => assert!(cfg.p6.domain_verbs.is_empty()),
            other => panic!("expected Loaded, got {other:?}"),
        }
    }

    #[test]
    fn invalid_when_domain_verbs_wrong_type() {
        let dir = unique_tmp("wrong-type");
        fs::write(
            dir.join(ANC_TOML_FILENAME),
            "[p6]\ndomain_verbs = \"post\"\n",
        )
        .expect("write .anc.toml");
        match load(&dir) {
            AncConfigLoad::Invalid(msg) => assert!(
                msg.starts_with("could not parse .anc.toml:"),
                "evidence message must start with the documented prefix; got: {msg}"
            ),
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn invalid_when_syntactically_broken() {
        let dir = unique_tmp("broken");
        fs::write(
            dir.join(ANC_TOML_FILENAME),
            "[p6\ndomain_verbs = [\"post\"]\n",
        )
        .expect("write .anc.toml");
        match load(&dir) {
            AncConfigLoad::Invalid(msg) => {
                assert!(msg.starts_with("could not parse .anc.toml:"), "got: {msg}");
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn absent_when_target_is_file() {
        let dir = unique_tmp("file-target");
        let bin = dir.join("tool");
        fs::write(&bin, "#!/bin/sh\necho hi\n").expect("write file");
        assert_eq!(load(&bin), AncConfigLoad::Absent);
    }

    #[test]
    fn as_config_fallback_is_none_for_non_loaded() {
        assert!(AncConfigLoad::Absent.as_config().is_none());
        assert!(
            AncConfigLoad::Invalid("could not parse .anc.toml: bad".into())
                .as_config()
                .is_none()
        );
    }

    #[test]
    fn as_config_returns_inner_for_loaded() {
        let cfg = AncConfig {
            p6: P6Config {
                domain_verbs: vec!["mentions".into()],
            },
        };
        let load = AncConfigLoad::Loaded(cfg);
        let got = load.as_config().expect("as_config returns inner");
        assert_eq!(got.p6.domain_verbs, vec!["mentions"]);
    }
}
