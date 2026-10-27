//! Callers waiting on an entry: the sender for its own answer, and a caller
//! that joins an identical request. Every wait ends by the caller's own
//! deadline, and a stage whose deadline passes while a caller waits is
//! recorded as timed out by that caller.

use std::sync::atomic::Ordering;
use std::sync::{Condvar, MutexGuard};
use std::time::Instant;

use super::stages::{Entry, Stages};
use super::{Shared, lock};
use crate::web_audit::fetch::hop::{self, HopContext, HopRecord};

#[derive(Clone, Copy)]
enum Stage {
    Headers,
    Full,
}

/// One caller counted in `MemoStats::waiting` for as long as it waits.
/// Held under the entry's lock, so it takes the memo's lock second.
struct Waiting<'a>(&'a Shared);

impl<'a> Waiting<'a> {
    fn count(shared: &'a Shared) -> Self {
        lock(&shared.state).stats.waiting += 1;
        Waiting(shared)
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        lock(&self.0.state).stats.waiting -= 1;
    }
}

/// How a joining caller fared.
pub(super) enum Joined {
    /// A record on hand answers the caller.
    Answer(HopRecord),
    /// The caller's deadline passed while it waited.
    TimedOut,
    /// Nothing on hand serves the caller; this is the best record there is.
    Wanting(HopRecord),
}

impl Entry {
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
        let _waiting = waited.then(|| Waiting::count(shared));
        loop {
            if let Some(record) = pick(&stages, stage) {
                return Some((record.clone(), waited));
            }
            if Instant::now() >= ctx.deadline {
                return None;
            }
            let until = ctx.deadline.min(self.due(&stages));
            stages = wait_until(&self.changed, stages, until);
            self.expire_due_locked(&mut stages, shared, Instant::now());
        }
    }

    /// The sender's own answer, waited for no longer than its deadline.
    pub(super) fn wait_sender(&self, shared: &Shared, ctx: &HopContext) -> HopRecord {
        let mut stages = self.stages();
        let mut waiting: Option<Waiting<'_>> = None;
        loop {
            self.expire_due_locked(&mut stages, shared, Instant::now());
            if let Some(record) = &stages.for_sender {
                return record.clone();
            }
            if Instant::now() >= ctx.deadline {
                return hop::timed_out_for(ctx);
            }
            waiting.get_or_insert_with(|| Waiting::count(shared));
            let until = ctx.deadline.min(self.due(&stages));
            stages = wait_until(&self.changed, stages, until);
        }
    }

    /// A caller joining this request: the headers when they serve it, else
    /// the full answer once the body read ends.
    pub(super) fn join(&self, shared: &Shared, ctx: &HopContext) -> Joined {
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
