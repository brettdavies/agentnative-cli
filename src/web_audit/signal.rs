//! Whether this build's vendored web-audit contract is the one anc.dev serves.
//!
//! A run reads the signal beside the audit, for a public target only, so a
//! local or private target's run never contacts anc.dev. An audit that outlasts
//! the read's deadline pays nothing for it, and a shorter one waits at most the
//! rest of that deadline, so the answer does not depend on how fast the target
//! is. Every failure reads as "not checked", never as an error: the audit does
//! not depend on the answer.

use std::fmt;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use super::fetch::{FetchInit, FetchOptions, Fetcher};
use super::locality::{Locality, Resolver, classify_host};
use super::registry::{AUDIT_USER_AGENT, CONTRACT_FINGERPRINT, SITE_SHA};
use super::transport::Transport;

/// Where anc.dev serves its web-audit identity.
pub const VERSION_SIGNAL_URL: &str = "https://anc.dev/api/web-audit/version";

const SIGNAL_TIMEOUT: Duration = Duration::from_secs(2);
const SIGNAL_MAX_BODY_BYTES: usize = 4 * 1024;

/// What a run learned about its contract against anc.dev's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractCheck {
    /// The served fingerprint equals the vendored one.
    Same,
    /// The served fingerprint differs. Either side can be the newer one: a
    /// pin tracks site `dev`, and anc.dev deploys from site `main`.
    Differs,
    /// No comparison was made.
    NotChecked(NotChecked),
}

/// Why no comparison was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotChecked {
    /// The target is local or private, so the run contacted no one else.
    NotPublic,
    /// The target's locality could not be established, so it counts as not
    /// public.
    ClassUnknown,
    /// The request produced no response.
    Unreachable,
    /// anc.dev answered with anything but 200.
    Status(u16),
    /// The answer was not readable JSON.
    Unreadable,
    /// The answer carried no `contract_fingerprint` of a fingerprint's shape.
    NoFingerprint,
    /// No answer arrived within the check's deadline.
    NoAnswer,
}

impl fmt::Display for NotChecked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotChecked::NotPublic => f.write_str("not a public target"),
            NotChecked::ClassUnknown => f.write_str("target locality unknown"),
            NotChecked::Unreachable => f.write_str("anc.dev unreachable"),
            NotChecked::Status(status) => write!(f, "anc.dev answered {status}"),
            NotChecked::Unreadable => f.write_str("unreadable answer"),
            NotChecked::NoFingerprint => f.write_str("no contract_fingerprint in the answer"),
            NotChecked::NoAnswer => f.write_str("no answer in time"),
        }
    }
}

impl fmt::Display for ContractCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContractCheck::Same => f.write_str("same"),
            ContractCheck::Differs => f.write_str("differs"),
            ContractCheck::NotChecked(reason) => write!(f, "not checked ({reason})"),
        }
    }
}

impl ContractCheck {
    /// The sentence a report carries when the contracts differ. It names both
    /// sides as possibly newer and offers no upgrade as the fix: the hash moves
    /// on any site release touching scoring, so the newest anc can differ with
    /// nothing newer to install.
    pub fn note(self) -> Option<String> {
        (self == ContractCheck::Differs).then(|| {
            format!(
                "this anc's web-audit contract (site {}) differs from the one anc.dev serves, \
                 so a verdict here can differ from anc.dev's; a newer anc release may carry \
                 anc.dev's contract, or anc.dev may not serve this one yet",
                &SITE_SHA[..7]
            )
        })
    }

    /// What a run prints on stderr when stdout carries only a scorecard or a
    /// row: the note unless quieted, and the result when verbose.
    pub fn stderr_lines(self, quiet: bool, verbose: bool) -> Vec<String> {
        let mut lines = Vec::new();
        if !quiet && let Some(note) = self.note() {
            lines.push(format!("warning: contract-differs: {note}"));
        }
        if verbose {
            lines.push(format!("verbose: anc.dev contract: {self}"));
        }
        lines
    }
}

/// A check running beside the audit.
#[derive(Debug)]
pub struct PendingCheck {
    answer: Receiver<ContractCheck>,
    started: Instant,
}

impl PendingCheck {
    /// Start the check on its own thread and return at once. An `http` target's
    /// run ends before any request goes out, so it starts no request here
    /// either.
    pub fn start(
        target_url: &str,
        transport: Arc<dyn Transport + Send + Sync>,
        resolver: Arc<dyn Resolver + Send + Sync>,
    ) -> Self {
        let host = url::Url::parse(target_url)
            .ok()
            .filter(|url| url.scheme() == "https")
            .and_then(|url| url.host_str().map(str::to_string));
        let (sender, answer) = mpsc::channel();
        thread::spawn(move || {
            let check = match host {
                Some(host) => check_contract(&host, transport.as_ref(), resolver.as_ref()),
                None => ContractCheck::NotChecked(NotChecked::ClassUnknown),
            };
            let _ = sender.send(check);
        });
        PendingCheck {
            answer,
            started: Instant::now(),
        }
    }

    /// The answer, waiting only for what is left of the deadline counted from
    /// [`PendingCheck::start`]. A stalled check's thread is abandoned, like a
    /// stalled worker.
    pub fn result(self) -> ContractCheck {
        let left = SIGNAL_TIMEOUT.saturating_sub(self.started.elapsed());
        self.answer
            .recv_timeout(left)
            .unwrap_or(ContractCheck::NotChecked(NotChecked::NoAnswer))
    }
}

/// Classify the target, then read and compare the signal for a public one.
pub fn check_contract(
    host: &str,
    transport: &dyn Transport,
    resolver: &dyn Resolver,
) -> ContractCheck {
    match classify_host(host, resolver) {
        Ok(Locality::Public) => read_signal(transport, resolver),
        Ok(_) => ContractCheck::NotChecked(NotChecked::NotPublic),
        Err(_) => ContractCheck::NotChecked(NotChecked::ClassUnknown),
    }
}

fn read_signal(transport: &dyn Transport, resolver: &dyn Resolver) -> ContractCheck {
    let answer = Fetcher::new(transport, resolver, AUDIT_USER_AGENT).fetch(
        VERSION_SIGNAL_URL,
        &FetchInit::default(),
        &FetchOptions {
            timeout: SIGNAL_TIMEOUT,
            max_body_bytes: Some(SIGNAL_MAX_BODY_BYTES),
            follow_redirects: false,
            ..FetchOptions::default()
        },
    );
    match answer.status {
        None => ContractCheck::NotChecked(NotChecked::Unreachable),
        Some(200) => compare(&answer.body, CONTRACT_FINGERPRINT),
        Some(status) => ContractCheck::NotChecked(NotChecked::Status(status)),
    }
}

fn compare(body: &str, vendored: &str) -> ContractCheck {
    let Ok(signal) = serde_json::from_str::<serde_json::Value>(body) else {
        return ContractCheck::NotChecked(NotChecked::Unreadable);
    };
    let served = signal
        .get("contract_fingerprint")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|served| is_fingerprint(served));
    match served {
        Some(served) if served == vendored => ContractCheck::Same,
        Some(_) => ContractCheck::Differs,
        None => ContractCheck::NotChecked(NotChecked::NoFingerprint),
    }
}

/// A value of no fingerprint's shape is a bad answer, not a different contract.
fn is_fingerprint(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surrounding_whitespace_on_the_served_value_still_compares_equal() {
        let body = r#"{"contract_fingerprint":"c06f1efd281b\n"}"#;
        assert_eq!(compare(body, "c06f1efd281b"), ContractCheck::Same);
    }

    #[test]
    fn extra_fields_are_ignored_and_a_different_value_differs() {
        let body = r#"{"spec_version":"0.7.0","contract_fingerprint":"aaaaaaaaaaaa"}"#;
        assert_eq!(compare(body, "bbbbbbbbbbbb"), ContractCheck::Differs);
    }

    #[test]
    fn a_signal_without_the_field_or_not_json_is_not_checked() {
        assert_eq!(
            compare(r#"{"registry_fingerprint":"c06f1efd281b"}"#, "c06f1efd281b"),
            ContractCheck::NotChecked(NotChecked::NoFingerprint)
        );
        assert_eq!(
            compare("<html>", "c06f1efd281b"),
            ContractCheck::NotChecked(NotChecked::Unreadable)
        );
    }

    #[test]
    fn a_value_of_no_fingerprints_shape_is_not_a_different_contract() {
        for served in ["", "   ", "unknown", "not-hex-at-all"] {
            let body = format!(r#"{{"contract_fingerprint":"{served}"}}"#);
            assert_eq!(
                compare(&body, "c06f1efd281b"),
                ContractCheck::NotChecked(NotChecked::NoFingerprint),
                "{served:?}"
            );
        }
    }

    #[test]
    fn the_note_says_differs_and_never_behind_or_upgrade_as_the_action() {
        let note = ContractCheck::Differs.note().expect("a note");
        assert!(note.contains("differs"), "{note}");
        assert!(!note.contains("behind"), "{note}");
        assert!(!note.to_lowercase().contains("upgrade"), "{note}");
        assert_eq!(ContractCheck::Same.note(), None);
        assert_eq!(
            ContractCheck::NotChecked(NotChecked::Unreachable).note(),
            None
        );
    }

    #[test]
    fn the_note_allows_for_either_side_being_the_newer_one() {
        let note = ContractCheck::Differs.note().expect("a note");
        assert!(note.contains("a newer anc release may carry"), "{note}");
        assert!(
            note.contains("anc.dev may not serve this one yet"),
            "{note}"
        );
    }

    #[test]
    fn stderr_carries_the_note_unless_quieted_and_the_result_when_verbose() {
        let note = ContractCheck::Differs.note().expect("a note");
        let warning = format!("warning: contract-differs: {note}");
        let differs = "verbose: anc.dev contract: differs".to_string();

        let lines = |check: ContractCheck, quiet, verbose| check.stderr_lines(quiet, verbose);
        assert_eq!(
            lines(ContractCheck::Differs, false, false),
            vec![warning.clone()]
        );
        assert_eq!(
            lines(ContractCheck::Differs, false, true),
            vec![warning, differs.clone()]
        );
        assert_eq!(lines(ContractCheck::Differs, true, true), vec![differs]);
        assert!(lines(ContractCheck::Differs, true, false).is_empty());
        assert!(lines(ContractCheck::Same, false, false).is_empty());
        assert_eq!(
            lines(ContractCheck::Same, false, true),
            vec!["verbose: anc.dev contract: same".to_string()]
        );
        assert_eq!(
            lines(
                ContractCheck::NotChecked(NotChecked::Status(404)),
                false,
                true
            ),
            vec!["verbose: anc.dev contract: not checked (anc.dev answered 404)".to_string()]
        );
    }
}
