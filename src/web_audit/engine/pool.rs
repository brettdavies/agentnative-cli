//! The bounded worker pool the waves run on.
//!
//! Blocking I/O cannot be cancelled from outside, so the deadline is enforced
//! where it can be: every request is issued with a timeout no longer than
//! the remaining budget, and the collector stops waiting when the budget is
//! gone. Workers are never joined; one stalled on a request that has its own
//! timeout finishes on its own and exits, and a worker that has not yet
//! started a job sees the cancellation flag and stops. A job that panics sets the
//! flag and panics the caller, so it does not read as a job the deadline cut short;
//! a panic past the deadline is as late as any other late job.
//!
//! ```text
//! caller                                     workers (N threads, detached)
//! ------                                     -----------------------------
//! queue = [job 0, job 1, ... job n-1]
//! spawn N ------------------------------->   loop:
//!                                              cancelled? -> exit
//!                                              pop job    -> none? exit
//!                                              run job (each request bounded
//!                                                by min(per-check, remaining))
//!                                              send (index, result)
//! loop until n results or the deadline:
//!   recv_timeout(deadline - now) <---------   (index, result)
//! past the deadline:
//!   cancelled = true; unreturned = None
//!   return                                     stalled workers finish their
//!                                              own timeout and exit
//! ```

use std::any::Any;
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

/// Run `job` over `items` on up to `concurrency` threads until every result
/// is in or `deadline` passes. The result vector keeps item order; a slot
/// is `None` when its job had not returned by the deadline.
pub fn run_bounded<T, R, F>(
    items: Vec<T>,
    concurrency: usize,
    deadline: Instant,
    job: F,
) -> Vec<Option<R>>
where
    T: Send + 'static,
    R: Send + 'static,
    F: Fn(T) -> R + Send + Sync + 'static,
{
    let total = items.len();
    let mut results: Vec<Option<R>> = (0..total).map(|_| None).collect();
    if total == 0 {
        return results;
    }
    let queue: Arc<Mutex<VecDeque<(usize, T)>>> =
        Arc::new(Mutex::new(items.into_iter().enumerate().collect()));
    let cancelled = Arc::new(AtomicBool::new(false));
    let job = Arc::new(job);
    let (tx, rx) = mpsc::channel::<(usize, Result<R, Box<dyn Any + Send>>)>();

    for _ in 0..concurrency.clamp(1, total) {
        let queue = Arc::clone(&queue);
        let cancelled = Arc::clone(&cancelled);
        let job = Arc::clone(&job);
        let tx = tx.clone();
        thread::spawn(move || {
            loop {
                if cancelled.load(Ordering::Relaxed) {
                    return;
                }
                let next = queue.lock().ok().and_then(|mut q| q.pop_front());
                let Some((index, item)) = next else { return };
                let result = catch_unwind(AssertUnwindSafe(|| job(item)));
                if result.is_err() {
                    cancelled.store(true, Ordering::Relaxed);
                }
                if tx.send((index, result)).is_err() {
                    return;
                }
            }
        });
    }
    drop(tx);

    let mut received = 0;
    while received < total {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match rx.recv_timeout(remaining) {
            Ok((_, Err(panic))) => resume_unwind(panic),
            Ok((index, Ok(result))) => {
                results[index] = Some(result);
                received += 1;
            }
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    cancelled.store(true, Ordering::Relaxed);
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn results_keep_item_order_across_threads() {
        let deadline = Instant::now() + Duration::from_secs(5);
        let out = run_bounded((0..20).collect(), 6, deadline, |n: u32| {
            thread::sleep(Duration::from_millis(u64::from(20 - n)));
            n * 2
        });
        let values: Vec<u32> = out.into_iter().map(|r| r.unwrap()).collect();
        assert_eq!(values, (0..20).map(|n| n * 2).collect::<Vec<_>>());
    }

    #[test]
    fn the_deadline_releases_the_caller_and_leaves_unreturned_slots_empty() {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(150);
        let out = run_bounded(vec![10u64, 2000, 10, 2000], 2, deadline, |ms: u64| {
            thread::sleep(Duration::from_millis(ms));
            ms
        });
        assert!(
            started.elapsed() < Duration::from_millis(1000),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(out[0], Some(10));
        assert_eq!(out[1], None);
        assert_eq!(out[3], None);
    }

    #[test]
    fn a_job_that_panics_panics_the_caller_with_its_own_message() {
        let panic = std::panic::catch_unwind(|| {
            let deadline = Instant::now() + Duration::from_secs(5);
            run_bounded(vec![0u8], 1, deadline, |_| panic!("job bug"))
        })
        .expect_err("the panic reaches the caller");
        assert_eq!(crate::panics::panic_message(&*panic), "job bug");
    }

    #[test]
    fn a_job_that_panics_stops_the_jobs_not_yet_started() {
        use std::sync::atomic::AtomicUsize;
        let started = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&started);
        let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let deadline = Instant::now() + Duration::from_secs(5);
            run_bounded((0..20).collect(), 1, deadline, move |_: u32| {
                counted.fetch_add(1, Ordering::SeqCst);
                panic!("job bug");
            })
        }));
        assert!(panic.is_err());
        thread::sleep(Duration::from_millis(50));
        assert_eq!(started.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn an_empty_wave_returns_immediately() {
        let out: Vec<Option<u8>> = run_bounded(Vec::<u8>::new(), 6, Instant::now(), |n| n);
        assert!(out.is_empty());
    }

    #[test]
    fn concurrency_is_bounded() {
        use std::sync::atomic::AtomicUsize;
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let (a, p) = (Arc::clone(&active), Arc::clone(&peak));
        let deadline = Instant::now() + Duration::from_secs(5);
        run_bounded((0..12).collect(), 3, deadline, move |_: u32| {
            let now = a.fetch_add(1, Ordering::SeqCst) + 1;
            p.fetch_max(now, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(20));
            a.fetch_sub(1, Ordering::SeqCst);
        });
        assert!(peak.load(Ordering::SeqCst) <= 3);
    }
}
