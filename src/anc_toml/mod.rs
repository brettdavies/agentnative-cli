//! `.anc.toml` loader — per-CLI configuration found by the audit target's
//! location.
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
//! Loader contract ([`load_for_target`]):
//!
//! - The files that apply form a [`chain::Chain`]: `~/.anc.toml`, then every
//!   `.anc.toml` from the target's repository root down to the target. A
//!   binary target starts from the directory holding it.
//! - No file anywhere returns [`AncConfigLoad::Absent`] — the loader is
//!   additive, never required.
//! - A file that cannot be read or parsed returns [`AncConfigLoad::Invalid`]
//!   naming that file, so audits can surface it in their evidence string;
//!   no setting from any other file applies.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::types::{ConfigFile, ConfigScope};

mod chain;

use chain::Chain;

/// Filename probed in each directory of the chain.
pub const ANC_TOML_FILENAME: &str = ".anc.toml";

/// Environment variable that relocates the user-level `~/.anc.toml`.
pub const HOME_CONFIG_ENV: &str = "AGENTNATIVE_HOME_CONFIG";

/// The README section that explains where anc looks for `.anc.toml`.
pub const DOCS_URL: &str = "https://github.com/brettdavies/agentnative-cli#configuration-anctoml";

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
#[derive(Debug, Default, PartialEq, Eq)]
pub enum AncConfigLoad {
    #[default]
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

/// How evidence names the user-level file when [`HOME_CONFIG_ENV`] is unset.
const DEFAULT_HOME_LABEL: &str = "~/.anc.toml";

/// The user-level layer: its file, and the name evidence gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeLayer {
    pub path: PathBuf,
    /// `~/.anc.toml`, or `$AGENTNATIVE_HOME_CONFIG` when the variable placed
    /// the file. Evidence lands in committed scorecards, so it names the
    /// variable rather than the path it holds.
    pub label: String,
    relocated: bool,
}

impl HomeLayer {
    /// The warning for a relocated file that does not exist. An override
    /// that names nothing is more often a typo than a choice; a missing
    /// default `~/.anc.toml` is the common case and stays silent.
    pub fn missing_warning(&self) -> Option<String> {
        (self.relocated && !self.path.exists()).then(|| {
            format!(
                "{HOME_CONFIG_ENV} names {}, which does not exist; no user-level .anc.toml applies",
                self.path.display()
            )
        })
    }
}

/// What `.anc.toml` gave an audit, and where a new setting for it belongs.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ResolvedConfig {
    /// The merged chain.
    pub load: AncConfigLoad,
    /// The files a new setting can go in, the repository's first. Empty for
    /// a `Project` built without resolving its config.
    pub settings_files: Vec<ConfigFile>,
}

/// The user-level layer: [`HOME_CONFIG_ENV`] when set, otherwise
/// `.anc.toml` in the home directory, and `None` without either.
pub fn home_layer() -> Option<HomeLayer> {
    home_layer_from(std::env::var_os(HOME_CONFIG_ENV), std::env::home_dir())
}

fn home_layer_from(var: Option<OsString>, home_dir: Option<PathBuf>) -> Option<HomeLayer> {
    match var {
        Some(path) if !path.is_empty() => Some(HomeLayer {
            path: PathBuf::from(path),
            label: format!("${HOME_CONFIG_ENV}"),
            relocated: true,
        }),
        _ => home_dir.map(|home| HomeLayer {
            path: home.join(ANC_TOML_FILENAME),
            label: DEFAULT_HOME_LABEL.to_string(),
            relocated: false,
        }),
    }
}

/// Resolve and load the config for an audit of `target`, a canonical path
/// from `Project::discover`. A file target starts the walk from its
/// directory; `repo` replaces the walk.
pub fn load_for_target(
    target: &Path,
    home: Option<&HomeLayer>,
    repo: Option<&Path>,
) -> ResolvedConfig {
    let start = match target.parent() {
        Some(dir) if target.is_file() => dir,
        _ => target,
    };
    let chain = chain::resolve(start, home.map(|layer| layer.path.as_path()), repo);
    let home_label = home.map_or(DEFAULT_HOME_LABEL, |layer| &layer.label);
    ResolvedConfig {
        load: load_chain(&chain, home_label),
        settings_files: settings_files(&chain, home_label),
    }
}

/// Where a new setting can go: `.anc.toml` at a repository root, scoped by
/// whether this audit found the repository, then the user-level file.
fn settings_files(chain: &Chain, home_label: &str) -> Vec<ConfigFile> {
    let repository = ConfigFile {
        file: ANC_TOML_FILENAME.to_string(),
        scope: if chain.repo_root.is_some() {
            ConfigScope::Repository
        } else {
            ConfigScope::ToolRepository
        },
    };
    let user = chain.home.as_ref().map(|_| ConfigFile {
        file: home_label.to_string(),
        scope: ConfigScope::User,
    });
    std::iter::once(repository).chain(user).collect()
}

/// Load every existing file in `chain` into one config, lowest precedence
/// first. A nearer file's list entries follow the ones already present, and
/// an entry that appears twice keeps its first position. Any file that
/// cannot be read or parsed voids the whole chain. Evidence names the
/// user-level file `home_label`.
fn load_chain(chain: &Chain, home_label: &str) -> AncConfigLoad {
    let mut merged: Option<AncConfig> = None;
    for file in chain.candidates() {
        let raw = match fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                let shown = display_path(chain, file, home_label);
                return AncConfigLoad::Invalid(format!("could not read .anc.toml at {shown}: {e}"));
            }
        };
        let cfg = match toml::from_str::<AncConfig>(&raw) {
            Ok(cfg) => cfg,
            Err(e) => {
                let shown = display_path(chain, file, home_label);
                return AncConfigLoad::Invalid(format!(
                    "could not parse .anc.toml at {shown}: {e}"
                ));
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
/// repo-relative, the user-level file is `home_label`, and any other file
/// shows only its directory's name.
fn display_path(chain: &Chain, file: &Path, home_label: &str) -> String {
    if chain.home.as_deref() == Some(file) {
        return home_label.to_string();
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

    #[test]
    fn the_variable_places_the_home_layer_and_names_it() {
        let layer = home_layer_from(Some("/elsewhere/anc.toml".into()), Some("/home/u".into()))
            .expect("layer");
        assert_eq!(layer.path, PathBuf::from("/elsewhere/anc.toml"));
        assert_eq!(layer.label, "$AGENTNATIVE_HOME_CONFIG");
    }

    #[test]
    fn without_the_variable_the_home_layer_is_in_the_home_directory() {
        for var in [None, Some(OsString::new())] {
            let layer = home_layer_from(var, Some("/home/u".into())).expect("layer");
            assert_eq!(layer.path, PathBuf::from("/home/u/.anc.toml"));
            assert_eq!(layer.label, "~/.anc.toml");
            assert_eq!(
                layer.missing_warning(),
                None,
                "a missing default stays silent"
            );
        }
        assert_eq!(home_layer_from(None, None), None);
    }

    #[test]
    fn a_relocated_layer_warns_only_when_its_file_is_missing() {
        let dir = unique_tmp("relocated");
        let present = dir.join("anc.toml");
        fs::write(&present, "").expect("write");
        let layer = home_layer_from(Some(present.into()), None).expect("layer");
        assert_eq!(layer.missing_warning(), None);

        let missing = dir.join("typo.toml");
        let layer = home_layer_from(Some(missing.clone().into()), None).expect("layer");
        let warning = layer.missing_warning().expect("warning");
        assert!(
            warning.starts_with("AGENTNATIVE_HOME_CONFIG names "),
            "{warning}"
        );
        assert!(
            warning.contains(&missing.display().to_string()),
            "{warning}"
        );
    }

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

        let load = load_chain(&chain::resolve(&cli, None, None), DEFAULT_HOME_LABEL);

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

        let load = load_chain(
            &chain::resolve(&cli, Some(&home_file), None),
            DEFAULT_HOME_LABEL,
        );

        assert_eq!(verbs(&load), ["repost", "post", "like", "quote"]);
    }

    #[test]
    fn file_without_p6_contributes_nothing() {
        let root = repo("merge-no-p6");
        let cli = root.join("cli");
        write(&root, "# no sections yet\n");
        write(&cli, "[p6]\ndomain_verbs = [\"like\"]\n");

        let load = load_chain(&chain::resolve(&cli, None, None), DEFAULT_HOME_LABEL);

        assert_eq!(verbs(&load), ["like"]);
    }

    #[test]
    fn broken_nested_file_voids_the_whole_chain() {
        let root = repo("merge-broken");
        let cli = root.join("crates/cli");
        write(&root, "[p6]\ndomain_verbs = [\"post\"]\n");
        write(&cli, "[p6]\ndomain_verbs = \"like\"\n");

        let msg = invalid(load_chain(
            &chain::resolve(&cli, None, None),
            DEFAULT_HOME_LABEL,
        ));

        assert!(
            msg.starts_with("could not parse .anc.toml at crates/cli/.anc.toml:"),
            "evidence must name the failing file repo-relative; got: {msg}"
        );
    }

    #[test]
    fn directory_named_anc_toml_is_invalid_and_named() {
        let root = repo("merge-dir");
        let cli = root.join("cli");
        write(&root, "[p6]\ndomain_verbs = [\"post\"]\n");
        fs::create_dir_all(cli.join(ANC_TOML_FILENAME)).expect("create directory named .anc.toml");

        let msg = invalid(load_chain(
            &chain::resolve(&cli, None, None),
            DEFAULT_HOME_LABEL,
        ));

        assert!(
            msg.starts_with("could not read .anc.toml at cli/.anc.toml:"),
            "got: {msg}"
        );
    }

    #[test]
    fn failing_home_file_displays_as_tilde() {
        let home = unique_tmp("merge-home-broken");
        let start = unique_tmp("merge-home-elsewhere");
        write(&home, "[p6\n");
        let home_file = home.join(ANC_TOML_FILENAME);

        let msg = invalid(load_chain(
            &chain::resolve(&start, Some(&home_file), None),
            DEFAULT_HOME_LABEL,
        ));

        assert!(
            msg.starts_with("could not parse .anc.toml at ~/.anc.toml:"),
            "got: {msg}"
        );
        assert!(!msg.contains(&*home.to_string_lossy()), "got: {msg}");
    }

    #[test]
    fn failing_file_outside_home_and_repo_displays_its_directory_basename() {
        let outside = unique_tmp("merge-outside").join("tool");
        write(&outside, "[p6\n");

        let msg = invalid(load_chain(
            &chain::resolve(&outside, None, None),
            DEFAULT_HOME_LABEL,
        ));

        assert!(
            msg.starts_with("could not parse .anc.toml at tool/.anc.toml:"),
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

        let load = load_chain(
            &chain::resolve(&cli, Some(&home_file), None),
            DEFAULT_HOME_LABEL,
        );

        assert_eq!(load, AncConfigLoad::Absent);
    }

    #[test]
    fn absent_when_no_file() {
        let dir = unique_tmp("absent");
        assert_eq!(
            load_for_target(&dir, None, None).load,
            AncConfigLoad::Absent
        );
    }

    #[test]
    fn loaded_with_domain_verbs() {
        let dir = unique_tmp("loaded");
        fs::write(
            dir.join(ANC_TOML_FILENAME),
            "[p6]\ndomain_verbs = [\"post\", \"like\"]\n",
        )
        .expect("write .anc.toml");
        match load_for_target(&dir, None, None).load {
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
        match load_for_target(&dir, None, None).load {
            AncConfigLoad::Loaded(cfg) => assert!(cfg.p6.domain_verbs.is_empty()),
            other => panic!("expected Loaded, got {other:?}"),
        }
    }

    #[test]
    fn loaded_without_p6_section() {
        let dir = unique_tmp("no-p6");
        fs::write(dir.join(ANC_TOML_FILENAME), "# empty config\n").expect("write .anc.toml");
        match load_for_target(&dir, None, None).load {
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
        match load_for_target(&dir, None, None).load {
            AncConfigLoad::Invalid(msg) => assert!(
                msg.starts_with("could not parse .anc.toml at "),
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
        match load_for_target(&dir, None, None).load {
            AncConfigLoad::Invalid(msg) => {
                assert!(
                    msg.starts_with("could not parse .anc.toml at "),
                    "got: {msg}"
                );
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn file_target_reads_the_config_beside_it() {
        let dir = unique_tmp("file-target");
        let bin = dir.join("tool");
        fs::write(&bin, "#!/bin/sh\necho hi\n").expect("write file");
        write(&dir, "[p6]\ndomain_verbs = [\"post\"]\n");

        assert_eq!(verbs(&load_for_target(&bin, None, None).load), ["post"]);
    }

    #[test]
    fn file_target_without_config_beside_it_is_absent() {
        let dir = unique_tmp("file-target-absent");
        let bin = dir.join("tool");
        fs::write(&bin, "#!/bin/sh\necho hi\n").expect("write file");

        assert_eq!(
            load_for_target(&bin, None, None).load,
            AncConfigLoad::Absent
        );
    }

    #[test]
    fn repo_replaces_the_walk_for_a_file_target() {
        let root = repo("target-repo");
        write(&root, "[p6]\ndomain_verbs = [\"post\"]\n");
        let bin = root.join("tool");
        fs::write(&bin, "#!/bin/sh\necho hi\n").expect("write file");
        let fetched = unique_tmp("target-fetched");
        write(&fetched, "[p6]\ndomain_verbs = [\"like\"]\n");

        let load = load_for_target(&bin, None, Some(&fetched)).load;

        assert_eq!(verbs(&load), ["like"]);
    }

    fn default_layer(file: &Path) -> HomeLayer {
        home_layer_from(None, file.parent().map(Path::to_path_buf)).expect("layer")
    }

    fn settings_files_for(
        start: &Path,
        home: Option<&Path>,
        repo: Option<&Path>,
    ) -> Vec<(String, ConfigScope)> {
        load_for_target(start, home.map(default_layer).as_ref(), repo)
            .settings_files
            .into_iter()
            .map(|file| (file.file, file.scope))
            .collect()
    }

    fn named(file: &str, scope: ConfigScope) -> (String, ConfigScope) {
        (file.to_string(), scope)
    }

    #[test]
    fn settings_go_at_the_repo_root_for_a_nested_target_or_in_the_home_file() {
        let root = repo("settings-repo");
        let cli = root.join("crates/cli");
        fs::create_dir_all(&cli).expect("create dir");
        let home_file = unique_tmp("settings-repo-home").join(ANC_TOML_FILENAME);

        assert_eq!(
            settings_files_for(&cli, Some(&home_file), None),
            [
                named(".anc.toml", ConfigScope::Repository),
                named("~/.anc.toml", ConfigScope::User),
            ]
        );
    }

    #[test]
    fn settings_outside_any_repo_go_in_the_tools_repo_or_the_home_file() {
        let start = unique_tmp("settings-outside");
        let home_file = unique_tmp("settings-outside-home").join(ANC_TOML_FILENAME);

        assert_eq!(
            settings_files_for(&start, Some(&home_file), None),
            [
                named(".anc.toml", ConfigScope::ToolRepository),
                named("~/.anc.toml", ConfigScope::User),
            ]
        );
    }

    #[test]
    fn settings_go_in_the_repo_flag_directory_or_the_home_file() {
        let start = unique_tmp("settings-flag-start");
        let fetched = unique_tmp("settings-flag-fetched");
        let home_file = unique_tmp("settings-flag-home").join(ANC_TOML_FILENAME);

        assert_eq!(
            settings_files_for(&start, Some(&home_file), Some(&fetched)),
            [
                named(".anc.toml", ConfigScope::Repository),
                named("~/.anc.toml", ConfigScope::User),
            ]
        );
    }

    #[test]
    fn settings_name_no_user_file_without_a_home_layer() {
        let start = unique_tmp("settings-bare").join("tool");
        fs::create_dir_all(&start).expect("create dir");

        assert_eq!(
            settings_files_for(&start, None, None),
            [named(".anc.toml", ConfigScope::ToolRepository)]
        );
    }

    #[test]
    fn a_relocated_home_layer_is_named_by_its_variable_in_settings() {
        let start = unique_tmp("settings-relocated");
        let file = unique_tmp("settings-relocated-home").join("anc.toml");
        let layer = home_layer_from(Some(file.into()), None).expect("layer");

        let files = load_for_target(&start, Some(&layer), None).settings_files;

        assert_eq!(files[1].file, "$AGENTNATIVE_HOME_CONFIG");
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
