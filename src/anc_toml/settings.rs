//! The settings `.anc.toml` carries: as one file writes them, and as the
//! chain merges them into [`AncConfig`].

use serde::Deserialize;

/// How evidence cites `[p5] confirm_flags`.
pub const CONFIRM_FLAGS_KEY: &str = "[p5].confirm_flags";

/// A merged setting and the file that supplied it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sourced<T> {
    pub value: T,
    /// The file, named the way evidence names files: repo-relative inside a
    /// repository, `~/.anc.toml` or `$AGENTNATIVE_HOME_CONFIG` for the
    /// user-level file, never an absolute path.
    pub file: String,
}

impl<T> Sourced<T> {
    /// The file and the setting, the way evidence cites them:
    /// `.anc.toml [p5].confirm_flags`.
    pub fn cite(&self, key: &str) -> String {
        format!("{} {key}", self.file)
    }
}

/// The settings of every file in the chain, merged.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct AncConfig {
    pub p5: P5Config,
    pub p6: P6Config,
}

/// `[p5]`: P5 (Safe Retries and Explicit Mutation Boundaries).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct P5Config {
    /// Flags that confirm a destructive subcommand non-interactively,
    /// accepted beside the built-in names.
    pub confirm_flags: Vec<Sourced<String>>,
}

/// `[p6]`: P6 (Composable and Predictable Command Structure).
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
pub struct P6Config {
    /// Per-CLI domain vocabulary that augments the global standard-verb list.
    /// Treated additively: a verb is recognized if it appears in the built-in
    /// list OR this slice.
    #[serde(default)]
    pub domain_verbs: Vec<String>,
}

/// One file's settings, as written. Unknown keys are ignored; a known key
/// of the wrong type fails the parse.
#[derive(Debug, Default, Deserialize)]
pub(super) struct FileConfig {
    #[serde(default)]
    p5: FileP5,
    #[serde(default)]
    p6: P6Config,
}

#[derive(Debug, Default, Deserialize)]
struct FileP5 {
    #[serde(default)]
    confirm_flags: Vec<String>,
}

impl AncConfig {
    /// Fold one file's settings onto the files of lower precedence already
    /// merged. `file` is how evidence names that file.
    pub(super) fn absorb(&mut self, settings: FileConfig, file: &str) {
        for verb in settings.p6.domain_verbs {
            if !self.p6.domain_verbs.contains(&verb) {
                self.p6.domain_verbs.push(verb);
            }
        }
        merge_list(&mut self.p5.confirm_flags, settings.p5.confirm_flags, file);
    }
}

/// Append each entry `merged` lacks. An entry already present keeps its
/// position and is credited to `file`, the nearer of the two, so evidence
/// names the repository's file over the user-level one.
fn merge_list(merged: &mut Vec<Sourced<String>>, entries: Vec<String>, file: &str) {
    for value in entries {
        match merged.iter_mut().find(|entry| entry.value == value) {
            Some(entry) => entry.file = file.to_string(),
            None => merged.push(Sourced {
                value,
                file: file.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> FileConfig {
        toml::from_str(raw).expect("valid settings")
    }

    fn flags(cfg: &AncConfig) -> Vec<(&str, &str)> {
        cfg.p5
            .confirm_flags
            .iter()
            .map(|flag| (flag.value.as_str(), flag.file.as_str()))
            .collect()
    }

    #[test]
    fn nearer_file_adds_flags_after_and_takes_credit_for_shared_ones() {
        let mut cfg = AncConfig::default();
        cfg.absorb(
            parse("[p5]\nconfirm_flags = [\"-auto-approve\", \"--nuke\"]\n"),
            "~/.anc.toml",
        );
        cfg.absorb(
            parse("[p5]\nconfirm_flags = [\"--nuke\", \"--really\"]\n"),
            ".anc.toml",
        );

        assert_eq!(
            flags(&cfg),
            [
                ("-auto-approve", "~/.anc.toml"),
                ("--nuke", ".anc.toml"),
                ("--really", ".anc.toml"),
            ]
        );
    }

    #[test]
    fn cite_names_the_file_then_the_setting() {
        let flag = Sourced {
            value: "-auto-approve".to_string(),
            file: "crates/cli/.anc.toml".to_string(),
        };
        assert_eq!(
            flag.cite(CONFIRM_FLAGS_KEY),
            "crates/cli/.anc.toml [p5].confirm_flags"
        );
    }
}
