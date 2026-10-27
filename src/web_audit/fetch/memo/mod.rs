//! The per-audit request memo, a port of the site engine's `request-hop.ts`.
//!
//! One audit sends each distinct request once. The memo is keyed on what the
//! target sees (method, URL, lowercased sorted headers, body) and consulted
//! once per redirect hop, so a caller's redirect or body-cap policy never
//! turns one question into two requests, and a redirect it holds still
//! passes the guard before a caller follows it. A request goes out again only
//! when the answer on record cannot serve the caller: it holds less of the
//! body than the caller reads, or it would not have reached the caller by
//! its deadline.
//!
//! ```text
//! caller A (sender)          hop thread (detached)          caller B (joins)
//! -----------------          ---------------------          ----------------
//! insert entry, spawn  --->  transport.send
//! wait <= A's deadline       status + headers -> headers    wait <= B's deadline
//!                            body read, deadline checked    headers serve B? return
//!                              between chunks -> full       else push the memo's
//!                                                             read back, wait full
//! ```
//!
//! Each sent hop runs on its own detached thread, and every caller, the
//! sender included, waits on the entry no longer than its own deadline. A
//! caller that skips the body (a status-only probe, a redirect it follows)
//! returns at the headers while the thread reads the body for a later
//! caller, up to [`MEMO_BODY_MAX_BYTES`] and under a deadline a later waiter
//! can push back. A stage whose deadline passes is recorded as timed out by
//! whoever sees it first, and a thread still blocked in a read is abandoned,
//! never joined: whatever it reads afterwards is dropped. The transport's
//! own timeout still bounds the request, so in production a memo-owned read
//! a later waiter pushed back still ends at the sender's budget, and that
//! waiter then sends its own request.
//!
//! The memo keeps the better of two answers, retains a body only within
//! [`MEMO_BODY_MAX_BYTES`] and the audit's [`MEMO_TOTAL_MAX_BYTES`], and
//! lives for one audit: nothing in it reaches the scorecard or the report.
//!
//! The parts: `key` is the identity of a request, `retention` decides what
//! the memo keeps, `stages` is one request's answer as it arrives, and
//! `waiters` is every caller waiting on one. Locks are always taken entry
//! first, memo second.

mod key;
mod retention;
mod stages;
mod waiters;

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Instant;

use super::hop::{self, HopContext, HopRecord};
use crate::web_audit::transport::{Request, Transport};
use key::Key;
pub use retention::{MEMO_BODY_MAX_BYTES, MEMO_TOTAL_MAX_BYTES};
use stages::{Entry, run_hop};
use waiters::Joined;

/// What one audit's memo did with the hops it saw and the bodies it kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoStats {
    /// Hops sent to the target.
    pub sent: usize,
    /// Hops answered from a record instead of sent.
    pub reused: usize,
    /// Body bytes kept against [`MEMO_TOTAL_MAX_BYTES`]; a record a better
    /// one replaced still counts.
    pub retained_bytes: usize,
    /// Non-empty bodies kept.
    pub bodies_retained: usize,
    /// The largest body kept.
    pub largest_body_bytes: usize,
    /// Bodies not kept for being over [`MEMO_BODY_MAX_BYTES`].
    pub over_body_cap: usize,
    /// Bodies not kept because the audit's total would pass [`MEMO_TOTAL_MAX_BYTES`].
    pub over_total_cap: usize,
    /// Body reads the memo runs on its own that have not ended.
    pub reads_open: usize,
    /// Callers waiting right now on a request in flight or a body read,
    /// senders included.
    pub waiting: usize,
}

/// One audit's answers, keyed by the request that produced them. Clones
/// share the same memo.
#[derive(Clone)]
pub struct RequestMemo {
    shared: Arc<Shared>,
}

struct Shared {
    state: Mutex<State>,
    ended: AtomicBool,
    digest: RandomState,
}

#[derive(Default)]
struct State {
    entries: HashMap<Key, Arc<Entry>>,
    captures: Vec<Arc<Entry>>,
    stats: MemoStats,
}

impl std::fmt::Debug for RequestMemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestMemo")
            .field("entries", &self.len())
            .field("ended", &self.is_ended())
            .finish_non_exhaustive()
    }
}

impl Default for RequestMemo {
    fn default() -> Self {
        Self::new()
    }
}

// A panic on a hop thread poisons nothing the memo cannot still read: every
// stage is written whole under the lock.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl RequestMemo {
    /// An empty memo for one audit.
    pub fn new() -> Self {
        RequestMemo {
            shared: Arc::new(Shared {
                state: Mutex::new(State::default()),
                ended: AtomicBool::new(false),
                digest: RandomState::new(),
            }),
        }
    }

    /// Requests on record.
    pub fn len(&self) -> usize {
        lock(&self.shared.state).entries.len()
    }

    /// Whether the memo holds no request.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether the audit has ended.
    pub fn is_ended(&self) -> bool {
        self.shared.ended.load(Ordering::SeqCst)
    }

    /// What the memo has done so far. A memo-owned read whose deadline has
    /// passed is recorded as timed out first.
    pub fn stats(&self) -> MemoStats {
        let captures = lock(&self.shared.state).captures.clone();
        let now = Instant::now();
        for entry in captures {
            entry.expire_due(&self.shared, now);
        }
        lock(&self.shared.state).stats
    }

    /// Release everything the memo holds once its audit ends: every request
    /// and body read still pending is recorded as timed out, every caller
    /// waiting on one returns, and every kept answer is dropped. Idempotent.
    pub fn close(&self) {
        let pending: Vec<Arc<Entry>> = {
            let mut state = lock(&self.shared.state);
            if self.shared.ended.swap(true, Ordering::SeqCst) {
                return;
            }
            let mut pending: Vec<Arc<Entry>> = state.entries.drain().map(|(_, e)| e).collect();
            pending.append(&mut state.captures);
            pending
        };
        for entry in pending {
            entry.expire_all(&self.shared, Instant::now());
        }
    }

    /// The answer to `request`: what an identical request already got this
    /// audit when that answers the caller, else whatever the target answers.
    /// A caller waits on an identical request in flight, or on the memo's
    /// read of its body, no longer than its own deadline, and pushes the
    /// memo's read back to that deadline. A new answer that serves less than
    /// the one on record goes to this caller only; the memo keeps the better.
    pub(crate) fn take(
        &self,
        transport: &Arc<dyn Transport + Send + Sync>,
        request: Request,
        ctx: HopContext,
    ) -> HopRecord {
        let key = Key::of(&request, &self.shared.digest);
        let mut prior: Option<HopRecord> = None;
        let mut wanting: Option<Arc<Entry>> = None;
        loop {
            let (entry, sends) = {
                let mut state = lock(&self.shared.state);
                if self.is_ended() {
                    return hop::timed_out_for(&ctx);
                }
                match state.entries.get(&key) {
                    Some(found) if !wanting.as_ref().is_some_and(|w| Arc::ptr_eq(w, found)) => {
                        (Arc::clone(found), false)
                    }
                    _ => {
                        let entry = Arc::new(Entry::new(prior.take(), ctx));
                        state.entries.insert(key.clone(), Arc::clone(&entry));
                        state.stats.sent += 1;
                        (entry, true)
                    }
                }
            };
            if sends {
                let (shared, sent, transport) = (
                    Arc::clone(&self.shared),
                    Arc::clone(&entry),
                    Arc::clone(transport),
                );
                thread::spawn(move || run_hop(&shared, &sent, &*transport, &request));
                return entry.wait_sender(&self.shared, &ctx);
            }
            match entry.join(&self.shared, &ctx) {
                Joined::Answer(record) => {
                    lock(&self.shared.state).stats.reused += 1;
                    return record;
                }
                Joined::TimedOut => return hop::timed_out_for(&ctx),
                Joined::Wanting(record) => {
                    if Instant::now() >= ctx.deadline {
                        return hop::timed_out_for(&ctx);
                    }
                    prior = Some(record);
                    wanting = Some(entry);
                }
            }
        }
    }
}
