//! The root fetch, running beside discovery's first document reads. What the
//! root turns out to be takes effect before anything waiting on it resumes.

use std::cell::OnceCell;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::types::FetchHandle;
use crate::web_audit::fetch::launch::Launch;
use crate::web_audit::fetch::memo::RequestMemo;
use crate::web_audit::fetch::{FetchInit, FetchOptions, ProbeResponse, Refusal};

pub(super) struct RootRead {
    answer: mpsc::Receiver<(ProbeResponse, Instant)>,
    settled: OnceCell<(ProbeResponse, Instant)>,
}

impl RootRead {
    pub(super) fn start(
        fetch: &FetchHandle,
        base: &str,
        timeout: Duration,
        memo: RequestMemo,
        launch: Launch,
    ) -> Self {
        let (fetch, base) = (fetch.clone(), base.to_string());
        let (tx, answer) = mpsc::channel();
        thread::spawn(move || {
            let _settled = RootSettled(launch.clone());
            let opts = FetchOptions {
                timeout,
                ..FetchOptions::default()
            };
            let response = fetch.fetch(&base, &FetchInit::default(), &opts);
            let settled_at = Instant::now();
            if response.refused == Some(Refusal::InsecureScheme) {
                // The first reads go out before the root's answer is read on
                // every https target, however fast it comes.
                launch.wait(timeout);
                memo.close();
            } else if response.status.is_none() {
                memo.cut();
            }
            let _ = tx.send((response, settled_at));
        });
        RootRead {
            answer,
            settled: OnceCell::new(),
        }
    }

    fn settled(&self) -> &(ProbeResponse, Instant) {
        self.settled.get_or_init(|| {
            self.answer.recv().unwrap_or_else(|_| {
                (
                    ProbeResponse::failed("root fetch panicked".to_string(), 0),
                    Instant::now(),
                )
            })
        })
    }

    pub(super) fn wait(&self) -> &ProbeResponse {
        &self.settled().0
    }

    pub(super) fn settled_at(&self) -> Instant {
        self.settled().1
    }
}

/// However the root fetch ends, a redirect hop waiting on it goes on.
struct RootSettled(Launch);

impl Drop for RootSettled {
    fn drop(&mut self) {
        self.0.root_settled();
    }
}
