//! Whether this build's vendored web-audit contract is the one anc.dev serves.
//!
//! A run reads the signal beside the audit, for a public target only, so it
//! costs the run no wall clock and a local or private target's run never
//! contacts anc.dev. Every failure reads as "not checked", never as an error:
//! the audit does not depend on the answer.

use std::fmt;
use std::io::Read;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use super::fetch::trust::TrustClass;
use super::locality::{Locality, Resolver, classify_host};
use super::registry::{CONTRACT_FINGERPRINT, SITE_SHA};
use super::transport::{Request, Transport};

/// Where anc.dev serves its web-audit identity.
pub const VERSION_SIGNAL_URL: &str = "https://anc.dev/api/web-audit/version";

const SIGNAL_TIMEOUT: Duration = Duration::from_secs(2);
const SIGNAL_MAX_BODY_BYTES: u64 = 4 * 1024;

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
    /// The answer carried no `contract_fingerprint`.
    NoFingerprint,
    /// The audit ended before the answer arrived.
    NoAnswerYet,
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
            NotChecked::NoAnswerYet => f.write_str("no answer before the run ended"),
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
    /// The sentence a report carries when the contracts differ. It offers no
    /// upgrade as the fix: the hash moves on any site release touching
    /// scoring, so the newest anc can differ with nothing newer to install.
    pub fn note(self) -> Option<String> {
        (self == ContractCheck::Differs).then(|| {
            format!(
                "this anc's web-audit contract (site {}) differs from the one anc.dev serves, \
                 so a verdict here can differ from anc.dev's; a newer anc release may carry \
                 anc.dev's contract",
                &SITE_SHA[..7]
            )
        })
    }
}

/// A check running beside the audit.
#[derive(Debug)]
pub struct PendingCheck(Receiver<ContractCheck>);

impl PendingCheck {
    /// Start the check on its own thread and return at once.
    pub fn start(
        target_url: &str,
        transport: Arc<dyn Transport + Send + Sync>,
        resolver: Arc<dyn Resolver + Send + Sync>,
    ) -> Self {
        let host = url::Url::parse(target_url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_string));
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let check = match host {
                Some(host) => check_contract(&host, transport.as_ref(), resolver.as_ref()),
                None => ContractCheck::NotChecked(NotChecked::ClassUnknown),
            };
            let _ = sender.send(check);
        });
        PendingCheck(receiver)
    }

    /// What the check has learned by now. Never waits, so a stalled signal
    /// adds nothing to the run; the thread is abandoned, like a stalled worker.
    pub fn result(&self) -> ContractCheck {
        self.0
            .try_recv()
            .unwrap_or(ContractCheck::NotChecked(NotChecked::NoAnswerYet))
    }
}

/// Classify the target, then read and compare the signal for a public one.
pub fn check_contract(
    host: &str,
    transport: &dyn Transport,
    resolver: &dyn Resolver,
) -> ContractCheck {
    match classify_host(host, resolver) {
        Ok(Locality::Public) => read_signal(transport),
        Ok(_) => ContractCheck::NotChecked(NotChecked::NotPublic),
        Err(_) => ContractCheck::NotChecked(NotChecked::ClassUnknown),
    }
}

fn read_signal(transport: &dyn Transport) -> ContractCheck {
    let request = Request {
        method: "GET".to_string(),
        url: VERSION_SIGNAL_URL.to_string(),
        headers: Vec::new(),
        body: None,
        timeout: SIGNAL_TIMEOUT,
        via_proxy: true,
        trust: TrustClass::Public,
    };
    let Ok(response) = transport.send(&request) else {
        return ContractCheck::NotChecked(NotChecked::Unreachable);
    };
    if response.status != 200 {
        return ContractCheck::NotChecked(NotChecked::Status(response.status));
    }
    let mut body = Vec::new();
    if response
        .body
        .take(SIGNAL_MAX_BODY_BYTES)
        .read_to_end(&mut body)
        .is_err()
    {
        return ContractCheck::NotChecked(NotChecked::Unreadable);
    }
    compare(&body, CONTRACT_FINGERPRINT)
}

fn compare(body: &[u8], vendored: &str) -> ContractCheck {
    let Ok(signal) = serde_json::from_slice::<serde_json::Value>(body) else {
        return ContractCheck::NotChecked(NotChecked::Unreadable);
    };
    match signal.get("contract_fingerprint").and_then(|v| v.as_str()) {
        Some(served) if served.trim() == vendored.trim() => ContractCheck::Same,
        Some(_) => ContractCheck::Differs,
        None => ContractCheck::NotChecked(NotChecked::NoFingerprint),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trailing_newline_on_the_vendored_file_still_compares_equal() {
        let body = br#"{"contract_fingerprint":"c06f1efd281b"}"#;
        assert_eq!(compare(body, "c06f1efd281b\n"), ContractCheck::Same);
    }

    #[test]
    fn extra_fields_are_ignored_and_a_different_value_differs() {
        let body = br#"{"spec_version":"0.7.0","contract_fingerprint":"aaaaaaaaaaaa"}"#;
        assert_eq!(compare(body, "bbbbbbbbbbbb\n"), ContractCheck::Differs);
    }

    #[test]
    fn a_signal_without_the_field_or_not_json_is_not_checked() {
        assert_eq!(
            compare(
                br#"{"registry_fingerprint":"c06f1efd281b"}"#,
                "c06f1efd281b\n"
            ),
            ContractCheck::NotChecked(NotChecked::NoFingerprint)
        );
        assert_eq!(
            compare(b"<html>", "c06f1efd281b\n"),
            ContractCheck::NotChecked(NotChecked::Unreadable)
        );
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
}
