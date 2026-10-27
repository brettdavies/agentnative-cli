//! One request's answer as it arrives, and every way a stage settles: the
//! hop thread publishing the headers and the body, a transport failure, or
//! a deadline passing before either arrived.
//!
//! ```text
//! in flight --- response ---> headers known --- body read ends ---> settled
//!     |                            |
//!     '-- failure, or the          '-- the read's deadline passes
//!         sender's deadline            (timed out; the blocked read
//!         passes (timed out)           is abandoned)
//! ```

use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Instant;

use super::retention::{MEMO_BODY_MAX_BYTES, better_of, retained};
use super::{Shared, lock};
use crate::web_audit::fetch::body;
use crate::web_audit::fetch::hop::{self, HopAnswer, HopContext, HopError, HopRecord};
use crate::web_audit::transport::{Request, Transport};

/// One request's answer: the headers stage, then the full stage once the
/// body read ends.
pub(super) struct Entry {
    /// The record on hand before this request went out, kept when the new
    /// one serves less.
    pub(super) prior: Option<HopRecord>,
    /// The sender's context: its deadline bounds the header phase.
    pub(super) sender: HopContext,
    pub(super) stages: Mutex<Stages>,
    pub(super) changed: Condvar,
}

#[derive(Default)]
pub(super) struct Stages {
    pub(super) head: Option<HopAnswer>,
    pub(super) headers: Option<HopRecord>,
    pub(super) full: Option<HopRecord>,
    /// What the sender itself gets: the head, or the body it read.
    pub(super) for_sender: Option<HopRecord>,
    /// The deadline of the body read in flight.
    pub(super) body_deadline: Option<Instant>,
    /// The body read is the memo's own, so a later waiter can push it back.
    pub(super) memo_owned: bool,
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

/// The detached thread behind one sent hop: send it, publish the headers,
/// then read the body for whoever needs it.
pub(super) fn run_hop(
    shared: &Shared,
    entry: &Arc<Entry>,
    transport: &dyn Transport,
    request: &Request,
) {
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
    pub(super) fn new(prior: Option<HopRecord>, sender: HopContext) -> Self {
        Entry {
            prior,
            sender,
            stages: Mutex::new(Stages::default()),
            changed: Condvar::new(),
        }
    }

    pub(super) fn stages(&self) -> MutexGuard<'_, Stages> {
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
        stages.full = Some(better_of(self.prior.as_ref(), kept));
        stages.body_deadline = None;
        if stages.for_sender.is_none() {
            stages.for_sender = Some(record);
        }
        self.changed.notify_all();
    }

    /// Record as timed out any stage whose deadline has passed by `now`.
    pub(super) fn expire_due_locked(&self, stages: &mut Stages, shared: &Shared, now: Instant) {
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

    pub(super) fn expire_due(&self, shared: &Shared, now: Instant) {
        let mut stages = self.stages();
        self.expire_due_locked(&mut stages, shared, now);
    }

    /// The audit ended: every stage still pending times out now.
    pub(super) fn expire_all(&self, shared: &Shared, now: Instant) {
        let mut stages = self.stages();
        if stages.headers.is_none() {
            let error = HopError::timeout_at(self.sender.started, now);
            self.settle_failure_locked(&mut stages, shared, error);
        } else if stages.full.is_none() {
            self.expire_body(&mut stages, shared, now);
        }
    }
}
