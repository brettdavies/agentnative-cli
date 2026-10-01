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
//!
//! [`load_chain`] reads every file of a [`chain::Chain`] and merges them; a
//! file that fails voids the chain, and the invalid outcome names that file.

use std::fs;
use std::path::Path;

use serde::Deserialize;

#[cfg_attr(not(test), expect(dead_code))]
pub mod chain;

use chain::Chain;

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

/// Load every existing file in `chain` into one config, lowest precedence
/// first. A nearer file's list entries follow the ones already present, and
/// an entry that appears twice keeps its first position. Any file that
/// cannot be read or parsed voids the whole chain.
#[cfg_attr(not(test), expect(dead_code))]
pub fn load_chain(chain: &Chain) -> AncConfigLoad {
    let mut merged: Option<AncConfig> = None;
    for file in chain.candidates() {
        let raw = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                let shown = display_path(chain, file);
                return AncConfigLoad::Invalid(format!("could not read {shown}: {e}"));
            }
        };
        let cfg = match toml::from_str::<AncConfig>(&raw) {
            Ok(cfg) => cfg,
            Err(e) => {
                let shown = display_path(chain, file);
                return AncConfigLoad::Invalid(format!("could not parse {shown}: {e}"));
            }
        };
        let verbs = &mut merged
            .get_or_insert_with(AncConfig::default)
            .p6
            .domain_verbs;
        for verb in cfg.p6.domain_verbs {
            if !verbs.contains(&verb) {
                verbs.push(verb);
            }
        }
    }
    merged.map_or(AncConfigLoad::Absent, AncConfigLoad::Loaded)
}

/// How evidence names a chain file. Evidence lands in committed scorecards,
/// so no directory above the repository may show: a repository file is
/// repo-relative, the user-level file is `~/.anc.toml`, and any other file
/// shows only its directory's name.
fn display_path(chain: &Chain, file: &Path) -> String {
    if chain.home.as_deref() == Some(file) {
        return format!("~/{ANC_TOML_FILENAME}");
    }
    if let Some(rel) = chain
        .repo_root
        .as_deref()
        .and_then(|root| file.strip_prefix(root).ok())
    {
        let parts: Vec<_> = rel.iter().map(|part| part.to_string_lossy()).collect();
        return parts.join("/");
    }
    match file.parent().and_then(Path::file_name) {
        Some(dir) => format!("{}/{ANC_TOML_FILENAME}", dir.to_string_lossy()),
        None => ANC_TOML_FILENAME.to_string(),
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

    fn repo(label: &str) -> std::path::PathBuf {
        let root = unique_tmp(label);
        fs::create_dir_all(root.join(".git")).expect("create .git");
        root
    }

    fn write(dir: &Path, body: &str) {
        fs::create_dir_all(dir).expect("create dir");
        fs::write(dir.join(ANC_TOML_FILENAME), body).expect("write .anc.toml");
    }

    fn verbs(load: &AncConfigLoad) -> Vec<&str> {
        match load {
            AncConfigLoad::Loaded(cfg) => cfg.p6.domain_verbs.iter().map(String::as_str).collect(),
            other => panic!("expected Loaded, got {other:?}"),
        }
    }

    fn invalid(load: AncConfigLoad) -> String {
        match load {
            AncConfigLoad::Invalid(msg) => msg,
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn nested_file_adds_its_verbs_after_the_root() {
        let root = repo("merge-nested");
        let cli = root.join("crates/cli");
        write(&root, "[p6]\ndomain_verbs = [\"post\"]\n");
        write(&cli, "[p6]\ndomain_verbs = [\"like\"]\n");

        let load = load_chain(&chain::resolve(&cli, None, None));

        assert_eq!(verbs(&load), ["post", "like"]);
    }

    #[test]
    fn verb_in_two_files_keeps_its_root_most_position() {
        let home = unique_tmp("merge-dupe-home");
        let root = repo("merge-dupe");
        let cli = root.join("cli");
        write(&home, "[p6]\ndomain_verbs = [\"repost\"]\n");
        write(&root, "[p6]\ndomain_verbs = [\"post\", \"like\"]\n");
        write(
            &cli,
            "[p6]\ndomain_verbs = [\"like\", \"repost\", \"quote\"]\n",
        );
        let home_file = home.join(ANC_TOML_FILENAME);

        let load = load_chain(&chain::resolve(&cli, Some(&home_file), None));

        assert_eq!(verbs(&load), ["repost", "post", "like", "quote"]);
    }

    #[test]
    fn file_without_p6_contributes_nothing() {
        let root = repo("merge-no-p6");
        let cli = root.join("cli");
        write(&root, "# no sections yet\n");
        write(&cli, "[p6]\ndomain_verbs = [\"like\"]\n");

        let load = load_chain(&chain::resolve(&cli, None, None));

        assert_eq!(verbs(&load), ["like"]);
    }

    #[test]
    fn broken_nested_file_voids_the_whole_chain() {
        let root = repo("merge-broken");
        let cli = root.join("crates/cli");
        write(&root, "[p6]\ndomain_verbs = [\"post\"]\n");
        write(&cli, "[p6]\ndomain_verbs = \"like\"\n");

        let msg = invalid(load_chain(&chain::resolve(&cli, None, None)));

        assert!(
            msg.starts_with("could not parse crates/cli/.anc.toml:"),
            "evidence must name the failing file repo-relative; got: {msg}"
        );
    }

    #[test]
    fn directory_named_anc_toml_is_invalid_and_named() {
        let root = repo("merge-dir");
        let cli = root.join("cli");
        write(&root, "[p6]\ndomain_verbs = [\"post\"]\n");
        fs::create_dir_all(cli.join(ANC_TOML_FILENAME)).expect("create directory named .anc.toml");

        let msg = invalid(load_chain(&chain::resolve(&cli, None, None)));

        assert!(
            msg.starts_with("could not read cli/.anc.toml:"),
            "got: {msg}"
        );
    }

    #[test]
    fn failing_home_file_displays_as_tilde() {
        let home = unique_tmp("merge-home-broken");
        let start = unique_tmp("merge-home-elsewhere");
        write(&home, "[p6\n");
        let home_file = home.join(ANC_TOML_FILENAME);

        let msg = invalid(load_chain(&chain::resolve(&start, Some(&home_file), None)));

        assert!(
            msg.starts_with("could not parse ~/.anc.toml:"),
            "got: {msg}"
        );
        assert!(!msg.contains(&*home.to_string_lossy()), "got: {msg}");
    }

    #[test]
    fn failing_file_outside_home_and_repo_displays_its_directory_basename() {
        let outside = unique_tmp("merge-outside").join("tool");
        write(&outside, "[p6\n");

        let msg = invalid(load_chain(&chain::resolve(&outside, None, None)));

        assert!(
            msg.starts_with("could not parse tool/.anc.toml:"),
            "got: {msg}"
        );
        let parent = outside.parent().expect("parent");
        assert!(!msg.contains(&*parent.to_string_lossy()), "got: {msg}");
    }

    #[test]
    fn chain_with_every_file_missing_is_absent() {
        let root = repo("merge-missing");
        let cli = root.join("cli");
        fs::create_dir_all(&cli).expect("create dir");
        let home_file = unique_tmp("merge-missing-home").join(ANC_TOML_FILENAME);

        let load = load_chain(&chain::resolve(&cli, Some(&home_file), None));

        assert_eq!(load, AncConfigLoad::Absent);
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
