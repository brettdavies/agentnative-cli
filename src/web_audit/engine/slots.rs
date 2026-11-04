//! One audit's concurrency cap, a mirror of the site's `audit-slots.ts`: a
//! fixed set of worker threads taking jobs lowest [`Rank`] first, and first come,
//! first served within a rank. Ranks only order what waits: a slot free when a
//! job arrives takes it whatever its rank. A check holds a slot while it probes;
//! a check gated without a request never takes one.
//!
//! A worker blocked on a request is never joined. Every request carries a
//! timeout no longer than the audit's remaining budget, so it finishes on its
//! own and the thread exits; [`AuditSlots::close`] drops the jobs still queued.

use std::any::Any;
use std::collections::{BTreeMap, VecDeque};
use std::panic::{self, AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;

type Job = Box<dyn FnOnce() + Send + 'static>;

/// Who a free slot goes to, lowest first. This decides which rows go unprobed
/// when the per-audit deadline passes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    /// A step every MCP row waits on.
    McpStep,
    /// A source check, which later rows read.
    Source,
    /// A row, by its registry index.
    Row(usize),
    /// A fan-out's extra probe.
    Helper,
}

#[derive(Default)]
struct Queue {
    jobs: BTreeMap<(Rank, u64), Job>,
    arrived: u64,
    closed: bool,
}

struct Inner {
    queue: Mutex<Queue>,
    ready: Condvar,
    cap: usize,
}

/// One audit's worker threads. Clones share them.
#[derive(Clone)]
pub struct AuditSlots {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for AuditSlots {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditSlots").finish_non_exhaustive()
    }
}

// A job runs outside the lock, so a panic cannot leave the queue half-written.
fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl AuditSlots {
    /// `cap` worker threads, at least one, waiting for jobs.
    pub fn new(cap: usize) -> Self {
        let cap = cap.max(1);
        let inner = Arc::new(Inner {
            queue: Mutex::new(Queue::default()),
            ready: Condvar::new(),
            cap,
        });
        for _ in 0..cap {
            let inner = Arc::clone(&inner);
            thread::spawn(move || work(&inner));
        }
        AuditSlots { inner }
    }

    /// Run `job` once a slot is free. A job submitted after [`Self::close`]
    /// never runs.
    pub fn submit(&self, rank: Rank, job: impl FnOnce() + Send + 'static) {
        let mut queue = locked(&self.inner.queue);
        if queue.closed {
            return;
        }
        let arrived = queue.arrived;
        queue.arrived += 1;
        queue.jobs.insert((rank, arrived), Box::new(job));
        drop(queue);
        self.inner.ready.notify_one();
    }

    /// Drop the jobs still queued and let every idle worker exit. A worker
    /// running a job exits when that job ends. Idempotent.
    pub fn close(&self) {
        let dropped = {
            let mut queue = locked(&self.inner.queue);
            queue.closed = true;
            std::mem::take(&mut queue.jobs)
        };
        drop(dropped);
        self.inner.ready.notify_all();
    }

    /// For a check already holding a slot: `probe` over `items` on that slot,
    /// joined by any other that frees while items remain, with results in
    /// item order however the probes finish. One row's fan-out always leaves a
    /// slot free, so it runs on at most `cap - 1` slots, and at a cap of 1 or 2 on
    /// its own alone. It never waits on a slot it does not yet have. A probe's
    /// panic comes back out here, once every probe under way has ended.
    pub fn fan_out<T, R>(
        &self,
        items: Vec<T>,
        probe: impl Fn(T) -> R + Send + Sync + 'static,
    ) -> Vec<R>
    where
        T: Send + 'static,
        R: Send + 'static,
    {
        let total = items.len();
        let fan = Arc::new(FanOut {
            state: Mutex::new(FanState {
                next: items.into_iter().enumerate().collect(),
                results: (0..total).map(|_| None).collect(),
                running: 0,
                panic: None,
            }),
            idle: Condvar::new(),
            probe,
        });
        let helpers = total
            .saturating_sub(1)
            .min(self.inner.cap.saturating_sub(2));
        for _ in 0..helpers {
            let helper = Arc::clone(&fan);
            self.submit(Rank::Helper, move || helper.drain());
        }
        fan.drain();
        fan.finish()
    }
}

fn work(inner: &Inner) {
    loop {
        let job = {
            let mut queue = locked(&inner.queue);
            loop {
                if queue.closed {
                    return;
                }
                if let Some((_, job)) = queue.jobs.pop_first() {
                    break job;
                }
                queue = inner
                    .ready
                    .wait(queue)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        };
        // Whatever a job owes the audit it reports itself; a panic here must
        // only not take the slot with it.
        let _ = catch_unwind(AssertUnwindSafe(job));
    }
}

struct FanState<T, R> {
    next: VecDeque<(usize, T)>,
    results: Vec<Option<R>>,
    running: usize,
    panic: Option<Box<dyn Any + Send>>,
}

struct FanOut<T, R, F> {
    state: Mutex<FanState<T, R>>,
    idle: Condvar,
    probe: F,
}

impl<T, R, F: Fn(T) -> R> FanOut<T, R, F> {
    /// Take items until none remain. A helper that gets its slot after that
    /// finds nothing to do and gives the slot straight back.
    fn drain(&self) {
        loop {
            let (index, item) = {
                let mut state = locked(&self.state);
                let Some(next) = state.next.pop_front() else {
                    return;
                };
                state.running += 1;
                next
            };
            let outcome = catch_unwind(AssertUnwindSafe(|| (self.probe)(item)));
            let mut state = locked(&self.state);
            state.running -= 1;
            match outcome {
                Ok(result) => state.results[index] = Some(result),
                Err(panic) => {
                    state.panic.get_or_insert(panic);
                }
            }
            drop(state);
            self.idle.notify_all();
        }
    }

    /// Wait out the helpers that started; one still queued does nothing when
    /// it runs, so nothing waits for it.
    fn finish(&self) -> Vec<R> {
        let mut state = locked(&self.state);
        while state.running > 0 {
            state = self
                .idle
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        if let Some(panic) = state.panic.take() {
            drop(state);
            panic::resume_unwind(panic);
        }
        state
            .results
            .iter_mut()
            .map(|slot| slot.take().expect("every item ran"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    #[test]
    fn jobs_within_a_rank_run_first_come_first_served() {
        let slots = AuditSlots::new(1);
        let (tx, rx) = mpsc::channel();
        for n in 0..8 {
            let tx = tx.clone();
            slots.submit(Rank::Source, move || tx.send(n).unwrap());
        }
        let seen: Vec<i32> = (0..8).map(|_| rx.recv().unwrap()).collect();
        assert_eq!(seen, (0..8).collect::<Vec<_>>());
        slots.close();
    }

    #[test]
    fn a_free_slot_goes_to_the_lowest_rank_waiting() {
        let slots = AuditSlots::new(1);
        let (hold_tx, hold_rx) = mpsc::channel::<()>();
        let (started_tx, started_rx) = mpsc::channel();
        slots.submit(Rank::Row(0), move || {
            started_tx.send(()).unwrap();
            let _ = hold_rx.recv();
        });
        started_rx.recv().unwrap();
        let (tx, rx) = mpsc::channel();
        for rank in [
            Rank::Helper,
            Rank::Row(9),
            Rank::Row(2),
            Rank::Source,
            Rank::McpStep,
        ] {
            let tx = tx.clone();
            slots.submit(rank, move || tx.send(rank).unwrap());
        }
        hold_tx.send(()).unwrap();
        let seen: Vec<Rank> = (0..5)
            .map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap())
            .collect();
        assert_eq!(
            seen,
            [
                Rank::McpStep,
                Rank::Source,
                Rank::Row(2),
                Rank::Row(9),
                Rank::Helper
            ]
        );
        slots.close();
    }

    #[test]
    fn a_slot_free_when_a_job_arrives_takes_it_whatever_its_rank() {
        let slots = AuditSlots::new(1);
        let (tx, rx) = mpsc::channel();
        slots.submit(Rank::Helper, move || tx.send(()).unwrap());
        rx.recv_timeout(Duration::from_secs(5)).expect("it ran");
        slots.close();
    }

    #[test]
    fn a_panicking_job_keeps_its_slot() {
        let slots = AuditSlots::new(1);
        let (tx, rx) = mpsc::channel();
        slots.submit(Rank::Source, || panic!("a job panics"));
        slots.submit(Rank::Source, move || tx.send(()).unwrap());
        rx.recv_timeout(Duration::from_secs(5)).expect("next job");
        slots.close();
    }

    #[test]
    fn close_drops_what_is_queued_and_refuses_what_comes_later() {
        let slots = AuditSlots::new(1);
        let (hold_tx, hold_rx) = mpsc::channel::<()>();
        let (started_tx, started_rx) = mpsc::channel();
        slots.submit(Rank::Source, move || {
            started_tx.send(()).unwrap();
            let _ = hold_rx.recv();
        });
        started_rx.recv().unwrap();
        let ran = Arc::new(AtomicUsize::new(0));
        let queued = Arc::clone(&ran);
        slots.submit(Rank::Source, move || {
            queued.fetch_add(1, Ordering::SeqCst);
        });
        slots.close();
        let late = Arc::clone(&ran);
        slots.submit(Rank::Source, move || {
            late.fetch_add(1, Ordering::SeqCst);
        });
        hold_tx.send(()).unwrap();
        thread::sleep(Duration::from_millis(30));
        assert_eq!(ran.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_fan_out_keeps_item_order_however_the_probes_finish() {
        let slots = AuditSlots::new(4);
        let out = slots.fan_out((0..8u64).collect(), |n| {
            thread::sleep(Duration::from_millis(8 - n));
            n * 2
        });
        assert_eq!(out, (0..8).map(|n| n * 2).collect::<Vec<_>>());
        slots.close();
    }

    #[test]
    fn a_fan_out_finishes_on_its_own_slot_at_a_cap_of_one() {
        let slots = AuditSlots::new(1);
        let inner = slots.clone();
        let (tx, rx) = mpsc::channel();
        slots.submit(Rank::Row(0), move || {
            tx.send(inner.fan_out(vec![1, 2, 3], |n| n + 1)).unwrap();
        });
        let out = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the fan-out never waits on a slot it does not hold");
        assert_eq!(out, vec![2, 3, 4]);
        slots.close();
    }

    #[test]
    fn a_fan_out_always_leaves_a_slot_free_and_uses_the_rest() {
        let cap = 4;
        let slots = AuditSlots::new(cap);
        let inner = slots.clone();
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let (r, p) = (Arc::clone(&running), Arc::clone(&peak));
        let (tx, rx) = mpsc::channel();
        slots.submit(Rank::Row(0), move || {
            inner.fan_out((0..12).collect::<Vec<u32>>(), move |_| {
                let now = r.fetch_add(1, Ordering::SeqCst) + 1;
                p.fetch_max(now, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(15));
                r.fetch_sub(1, Ordering::SeqCst);
            });
            tx.send(()).unwrap();
        });
        rx.recv_timeout(Duration::from_secs(5)).expect("finished");
        assert_eq!(peak.load(Ordering::SeqCst), cap - 1);
        slots.close();
    }

    #[test]
    fn a_probe_panic_comes_back_out_of_the_fan_out_after_the_rest_end() {
        let slots = AuditSlots::new(3);
        let inner = slots.clone();
        let (tx, rx) = mpsc::channel();
        slots.submit(Rank::Row(0), move || {
            let caught = catch_unwind(AssertUnwindSafe(|| {
                inner.fan_out(vec![0, 1, 2, 3], |n: i32| {
                    assert_ne!(n, 2, "probe two panics");
                    n
                })
            }));
            tx.send(caught.is_err()).unwrap();
        });
        let panicked = rx.recv_timeout(Duration::from_secs(5)).expect("finished");
        assert!(panicked);
        slots.close();
    }
}
