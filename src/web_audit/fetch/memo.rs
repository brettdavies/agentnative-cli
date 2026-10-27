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

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::Instant;

use super::body;
use super::hop::{self, HopAnswer, HopContext, HopError, HopRecord};
use crate::web_audit::transport::{Request, Transport};

/// The largest body the memo keeps for later callers, and the cap on a body
/// it reads on its own. The site engine's value.
pub const MEMO_BODY_MAX_BYTES: usize = 1024 * 1024;
/// The most body bytes one audit's memo keeps. The site engine's value.
pub const MEMO_TOTAL_MAX_BYTES: usize = 8 * 1024 * 1024;

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

/// What the target sees of a request. An `Authorization` value enters as a
/// digest, so the key never holds a credential, and the key prints header
/// names only.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(n, _)| n.as_str()).collect();
        f.debug_struct("Key")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &names)
            .finish_non_exhaustive()
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

    fn key_of(&self, request: &Request) -> Key {
        let mut headers: Vec<(String, String)> = request
            .headers
            .iter()
            .map(|(name, value)| {
                let name = name.to_ascii_lowercase();
                let value = if name == "authorization" {
                    format!("digest:{:016x}", self.shared.digest.hash_one(value))
                } else {
                    value.clone()
                };
                (name, value)
            })
            .collect();
        headers.sort_by(|a, b| a.0.cmp(&b.0));
        Key {
            method: request.method.to_ascii_uppercase(),
            url: request.url.clone(),
            headers,
            body: request.body.clone(),
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
        let key = self.key_of(&request);
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

/// The body read a sent hop runs after the headers.
enum BodyPlan {
    /// The header phase had already timed out; the response is dropped.
    Abandoned,
    /// Nobody reads the body.
    Skip,
    /// Read the body up to this cap.
    Read(Option<usize>),
}

/// How a joining caller fared.
enum Joined {
    Answer(HopRecord),
    TimedOut,
    Wanting(HopRecord),
}

/// One request's answer as it arrives: the headers stage, then the full
/// stage once the body read ends.
struct Entry {
    /// The record on hand before this request went out, kept when the new
    /// one serves less.
    prior: Option<HopRecord>,
    /// The sender's context: its deadline bounds the header phase.
    sender: HopContext,
    stages: Mutex<Stages>,
    changed: Condvar,
}

#[derive(Default)]
struct Stages {
    head: Option<HopAnswer>,
    headers: Option<HopRecord>,
    full: Option<HopRecord>,
    /// What the sender itself gets: the head, or the body it read.
    for_sender: Option<HopRecord>,
    /// The deadline of the body read in flight.
    body_deadline: Option<Instant>,
    /// The body read is the memo's own, so a later waiter can push it back.
    memo_owned: bool,
}

#[derive(Clone, Copy)]
enum Stage {
    Headers,
    Full,
}

fn run_hop(shared: &Shared, entry: &Arc<Entry>, transport: &dyn Transport, request: &Request) {
    let ctx = entry.sender;
    let response = match transport.send(request) {
        Ok(response) => response,
        Err(err) => {
            entry.settle_failure(shared, HopError::from_transport(&err, &ctx));
            return;
        }
    };
    let head = HopAnswer::head(response.status, response.headers, ctx.started);
    let cap = match entry.publish_head(shared, head) {
        BodyPlan::Read(cap) => cap,
        BodyPlan::Skip | BodyPlan::Abandoned => return,
    };
    let read = body::read_capped_until(response.body, cap, || entry.body_in_time());
    entry.settle_body(shared, read, cap);
}

impl Entry {
    fn new(prior: Option<HopRecord>, sender: HopContext) -> Self {
        Entry {
            prior,
            sender,
            stages: Mutex::new(Stages::default()),
            changed: Condvar::new(),
        }
    }

    fn stages(&self) -> MutexGuard<'_, Stages> {
        lock(&self.stages)
    }

    /// The status and headers arrived: publish them and decide who reads the body.
    fn publish_head(self: &Arc<Self>, shared: &Shared, head: HopAnswer) -> BodyPlan {
        let mut stages = self.stages();
        if stages.headers.is_some() {
            return BodyPlan::Abandoned;
        }
        stages.head = Some(head.clone());
        stages.headers = Some(HopRecord::Answer(head.clone()));
        let plan = if hop::reads_body(head.status, &head.headers, &self.sender) {
            stages.body_deadline = Some(self.sender.deadline);
            BodyPlan::Read(self.sender.body_cap)
        } else {
            stages.for_sender = Some(HopRecord::Answer(head.clone()));
            let mut state = lock(&shared.state);
            if shared.ended.load(Ordering::SeqCst) {
                drop(state);
                self.settle_full(&mut stages, shared, HopRecord::Answer(head));
                BodyPlan::Skip
            } else {
                state.captures.push(Arc::clone(self));
                state.stats.reads_open += 1;
                stages.body_deadline = Some(self.sender.deadline);
                stages.memo_owned = true;
                BodyPlan::Read(Some(MEMO_BODY_MAX_BYTES))
            }
        };
        self.changed.notify_all();
        plan
    }

    /// Whether the body read may go on: its stage is still open and its
    /// deadline, as later waiters moved it, has not passed.
    fn body_in_time(&self) -> bool {
        let stages = self.stages();
        stages.full.is_none() && stages.body_deadline.is_some_and(|d| Instant::now() < d)
    }

    fn settle_body(
        &self,
        shared: &Shared,
        read: std::io::Result<body::BodyRead>,
        cap: Option<usize>,
    ) {
        let mut stages = self.stages();
        let Some(head) = stages.head.clone() else {
            return;
        };
        if stages.full.is_some() {
            return;
        }
        let record = match read {
            Ok(read) => head.with_body(read, cap),
            Err(err) => {
                let mut ctx = self.sender;
                ctx.deadline = stages.body_deadline.unwrap_or(ctx.deadline);
                head.with_body_error(HopError::from_body(err, &ctx))
            }
        };
        self.settle_full(&mut stages, shared, HopRecord::Answer(record));
    }

    /// No response came: the failure answers the sender, and the headers
    /// stage serves whichever of it and the prior record serves more.
    fn settle_failure(&self, shared: &Shared, error: HopError) {
        let mut stages = self.stages();
        self.settle_failure_locked(&mut stages, shared, error);
    }

    fn settle_failure_locked(&self, stages: &mut Stages, shared: &Shared, error: HopError) {
        if stages.headers.is_some() {
            return;
        }
        self.settle_full(stages, shared, HopRecord::Failure(error));
        stages.headers = stages.full.clone();
    }

    /// The body read ended: keep the better of this record and the prior one.
    fn settle_full(&self, stages: &mut Stages, shared: &Shared, record: HopRecord) {
        let kept = {
            let mut state = lock(&shared.state);
            if stages.memo_owned {
                state.stats.reads_open -= 1;
                state
                    .captures
                    .retain(|e| !std::ptr::eq(Arc::as_ptr(e), self));
            }
            retained(&record, &mut state.stats)
        };
        let better = match &self.prior {
            Some(prior) if hop::coverage(prior) > hop::coverage(&kept) => prior.clone(),
            _ => kept,
        };
        stages.full = Some(better);
        stages.body_deadline = None;
        if stages.for_sender.is_none() {
            stages.for_sender = Some(record);
        }
        self.changed.notify_all();
    }

    /// Record as timed out any stage whose deadline has passed by `now`.
    fn expire_due_locked(&self, stages: &mut Stages, shared: &Shared, now: Instant) {
        if stages.headers.is_none() {
            if now >= self.sender.deadline {
                let error = HopError::timeout_at(self.sender.started, self.sender.deadline);
                self.settle_failure_locked(stages, shared, error);
            }
            return;
        }
        if stages.full.is_none()
            && let Some(due) = stages.body_deadline
            && now >= due
        {
            self.expire_body(stages, shared, due);
        }
    }

    fn expire_body(&self, stages: &mut Stages, shared: &Shared, at: Instant) {
        if let Some(head) = stages.head.clone() {
            let record = head.with_body_error(HopError::timeout_at(head.started, at));
            self.settle_full(stages, shared, HopRecord::Answer(record));
        }
    }

    fn expire_due(&self, shared: &Shared, now: Instant) {
        let mut stages = self.stages();
        self.expire_due_locked(&mut stages, shared, now);
    }

    /// The audit ended: every stage still pending times out now.
    fn expire_all(&self, shared: &Shared, now: Instant) {
        let mut stages = self.stages();
        if stages.headers.is_none() {
            let error = HopError::timeout_at(self.sender.started, now);
            self.settle_failure_locked(&mut stages, shared, error);
        } else if stages.full.is_none() {
            self.expire_body(&mut stages, shared, now);
        }
    }

    /// When the stage settles on its own if nothing arrives: the header
    /// phase at the sender's deadline, a body read at its own.
    fn due(&self, stages: &Stages) -> Instant {
        if stages.headers.is_none() {
            return self.sender.deadline;
        }
        stages.body_deadline.unwrap_or(self.sender.deadline)
    }

    /// Wait for a stage no longer than the caller's deadline; whether the
    /// caller had to wait for it comes back with the record.
    fn settle(&self, shared: &Shared, stage: Stage, ctx: &HopContext) -> Option<(HopRecord, bool)> {
        let mut stages = self.stages();
        self.expire_due_locked(&mut stages, shared, Instant::now());
        let waited = pick(&stages, stage).is_none();
        loop {
            if let Some(record) = pick(&stages, stage) {
                return Some((record.clone(), waited));
            }
            let now = Instant::now();
            if now >= ctx.deadline {
                return None;
            }
            let until = ctx.deadline.min(self.due(&stages));
            stages = wait_until(&self.changed, stages, until);
            self.expire_due_locked(&mut stages, shared, Instant::now());
        }
    }

    /// The sender's own answer, waited for no longer than its deadline.
    fn wait_sender(&self, shared: &Shared, ctx: &HopContext) -> HopRecord {
        let mut stages = self.stages();
        loop {
            self.expire_due_locked(&mut stages, shared, Instant::now());
            if let Some(record) = &stages.for_sender {
                return record.clone();
            }
            if Instant::now() >= ctx.deadline {
                return hop::timed_out_for(ctx);
            }
            let until = ctx.deadline.min(self.due(&stages));
            stages = wait_until(&self.changed, stages, until);
        }
    }

    /// A caller joining this request: the headers when they serve it, else
    /// the full answer once the body read ends.
    fn join(&self, shared: &Shared, ctx: &HopContext) -> Joined {
        let Some((head, waited)) = self.settle(shared, Stage::Headers, ctx) else {
            return Joined::TimedOut;
        };
        if hop::answers(&head, waited, ctx) {
            return Joined::Answer(head);
        }
        self.extend(shared, ctx.deadline);
        let Some((full, waited)) = self.settle(shared, Stage::Full, ctx) else {
            return Joined::TimedOut;
        };
        if hop::answers(&full, waited, ctx) {
            return Joined::Answer(full);
        }
        Joined::Wanting(full)
    }

    /// Push the memo's own body read back to a later deadline. A caller
    /// still waiting when the audit ends cannot push it past that moment.
    fn extend(&self, shared: &Shared, deadline: Instant) {
        let mut stages = self.stages();
        if !stages.memo_owned || stages.full.is_some() || shared.ended.load(Ordering::SeqCst) {
            return;
        }
        if stages.body_deadline.is_some_and(|d| d < deadline) {
            stages.body_deadline = Some(deadline);
            self.changed.notify_all();
        }
    }
}

fn pick(stages: &Stages, stage: Stage) -> Option<&HopRecord> {
    match stage {
        Stage::Headers => stages.headers.as_ref(),
        Stage::Full => stages.full.as_ref(),
    }
}

fn wait_until<'a>(
    changed: &Condvar,
    guard: MutexGuard<'a, Stages>,
    until: Instant,
) -> MutexGuard<'a, Stages> {
    let timeout = until.saturating_duration_since(Instant::now());
    match changed.wait_timeout(guard, timeout) {
        Ok((guard, _)) => guard,
        Err(poisoned) => poisoned.into_inner().0,
    }
}

/// The record the memo keeps for later callers: the body only while it fits
/// under both caps.
fn retained(record: &HopRecord, stats: &mut MemoStats) -> HopRecord {
    let HopRecord::Answer(answer) = record else {
        return record.clone();
    };
    let size = answer.body_len();
    if size > MEMO_BODY_MAX_BYTES {
        stats.over_body_cap += 1;
    } else if stats.retained_bytes + size > MEMO_TOTAL_MAX_BYTES {
        stats.over_total_cap += 1;
    } else {
        stats.retained_bytes += size;
        if size > 0 {
            stats.bodies_retained += 1;
        }
        stats.largest_body_bytes = stats.largest_body_bytes.max(size);
        return record.clone();
    }
    HopRecord::Answer(answer.without_body())
}
