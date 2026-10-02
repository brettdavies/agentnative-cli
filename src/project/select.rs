//! Choosing the binary a directory audit grades.

use std::ffi::OsString;
use std::path::Path;

use serde::Serialize;

use super::bins::Candidate;
use crate::argv::format_invocation;

/// Why a directory audit cannot choose its binary.
#[derive(Debug, PartialEq, Eq)]
pub enum Unselected {
    /// Several candidates and no `--bin`, or a `--bin` name several share.
    Ambiguous,
    /// `--bin` names no candidate.
    Unknown(String),
}

/// The candidate to grade: the only one, or the one `bin` names by its bin
/// name or by its path relative to `root`. `None` when there is nothing to
/// grade.
pub fn select<'a>(
    candidates: &'a [Candidate],
    root: &Path,
    bin: Option<&str>,
) -> Result<Option<&'a Candidate>, Unselected> {
    let matching: Vec<&Candidate> = match bin {
        None => candidates.iter().collect(),
        Some(bin) => {
            let bin = bin.trim_start_matches("./").replace('\\', "/");
            candidates
                .iter()
                .filter(|c| c.name == bin || relative_path(root, &c.path) == bin)
                .collect()
        }
    };
    match (matching.as_slice(), bin) {
        ([], None) => Ok(None),
        ([], Some(bin)) => Err(Unselected::Unknown(bin.to_string())),
        ([one], _) => Ok(Some(*one)),
        _ => Err(Unselected::Ambiguous),
    }
}

/// One way to finish an ambiguous audit: the candidate and the command that
/// grades it.
#[derive(Debug, Serialize)]
pub struct Choice {
    pub name: String,
    pub package: String,
    /// The built binary, relative to the audit root.
    pub path: String,
    /// The caller's invocation with `--bin <name>`, runnable as printed.
    pub command: String,
}

/// A [`Choice`] for every candidate. `rerun` is the caller's invocation
/// without `--bin` ([`crate::argv::rerun_argv`]); each command adds its own
/// `--bin`, so it keeps every other flag the caller passed. A candidate
/// whose name another shares is named by its path.
pub fn choices(candidates: &[Candidate], root: &Path, rerun: &[OsString]) -> Vec<Choice> {
    candidates
        .iter()
        .map(|candidate| {
            let path = relative_path(root, &candidate.path);
            let shared = candidates
                .iter()
                .filter(|c| c.name == candidate.name)
                .count()
                > 1;
            let bin = if shared {
                path.clone()
            } else {
                candidate.name.clone()
            };
            let mut args = rerun.to_vec();
            args.extend([OsString::from("--bin"), OsString::from(bin)]);
            Choice {
                name: candidate.name.clone(),
                package: candidate.package.clone(),
                path,
                command: format_invocation(&args),
            }
        })
        .collect()
}

/// `path` relative to `root`, with forward slashes.
fn relative_path(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let parts: Vec<_> = rel.iter().map(|part| part.to_string_lossy()).collect();
    parts.join("/")
}
