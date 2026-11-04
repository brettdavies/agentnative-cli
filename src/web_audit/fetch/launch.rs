//! Discovery's first reads, going out beside the root fetch, so that what any
//! of them answers cannot decide whether another is sent. A read counts as out
//! once the memo has taken it, or once it has been refused and nothing will send
//! it. Its redirect hops wait for the root, whose fate decides whether they go.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
struct Count {
    /// `None` until the group's size is known.
    left: Mutex<Option<usize>>,
    all_out: Condvar,
    root_settled: Mutex<bool>,
    root_changed: Condvar,
}

/// Clones share the count.
#[derive(Clone, Debug, Default)]
pub struct Launch {
    count: Arc<Count>,
}

impl PartialEq for Launch {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.count, &other.count)
    }
}

impl Eq for Launch {}

impl Launch {
    /// A group whose size is not known yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The group holds this many requests, none of them out yet.
    pub fn expect(&self, requests: usize) {
        *self.left() = Some(requests);
        self.count.all_out.notify_all();
    }

    /// Wait until every request in the group is out, for no longer than
    /// `at_most`.
    pub fn wait(&self, at_most: Duration) {
        let until = Instant::now() + at_most;
        let mut left = self.left();
        while *left != Some(0) {
            let timeout = until.saturating_duration_since(Instant::now());
            if timeout.is_zero() {
                return;
            }
            left = match self.count.all_out.wait_timeout(left, timeout) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }

    /// The root has settled, and what it means for the reads has taken effect.
    pub fn root_settled(&self) {
        *self.settled() = true;
        self.count.root_changed.notify_all();
    }

    pub(crate) fn wait_for_root(&self) {
        let mut settled = self.settled();
        while !*settled {
            settled = self
                .count
                .root_changed
                .wait(settled)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    fn settled(&self) -> std::sync::MutexGuard<'_, bool> {
        self.count
            .root_settled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn boarding(&self) -> Boarding {
        Boarding {
            launch: self.clone(),
            out: AtomicBool::new(false),
        }
    }

    fn left(&self) -> std::sync::MutexGuard<'_, Option<usize>> {
        self.count
            .left
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn one_out(&self) {
        let mut left = self.left();
        if let Some(left) = left.as_mut() {
            *left = left.saturating_sub(1);
        }
        drop(left);
        self.count.all_out.notify_all();
    }
}

/// One request of a group. It counts as out at most once, and no later than
/// when this drops, so a request refused before reaching the memo still counts.
#[derive(Debug)]
pub(crate) struct Boarding {
    launch: Launch,
    out: AtomicBool,
}

impl Boarding {
    pub(crate) fn out(&self) {
        if !self.out.swap(true, Ordering::SeqCst) {
            self.launch.one_out();
        }
    }

    pub(crate) fn wait_for_root(&self) {
        self.launch.wait_for_root();
    }
}

impl Drop for Boarding {
    fn drop(&mut self) {
        self.out();
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn it_waits_until_every_request_is_out() {
        let launch = Launch::new();
        launch.expect(2);
        let (first, second) = (launch.boarding(), launch.boarding());
        first.out();
        let waiter = {
            let launch = launch.clone();
            thread::spawn(move || {
                let started = Instant::now();
                launch.wait(Duration::from_secs(5));
                started.elapsed()
            })
        };
        thread::sleep(Duration::from_millis(40));
        second.out();
        let waited = waiter.join().unwrap();
        assert!(waited >= Duration::from_millis(30), "{waited:?}");
        assert!(waited < Duration::from_secs(5), "{waited:?}");
    }

    #[test]
    fn a_request_counts_once_and_a_dropped_one_counts_too() {
        let launch = Launch::new();
        launch.expect(2);
        let first = launch.boarding();
        first.out();
        first.out();
        assert_eq!(*launch.left(), Some(1));
        drop(first);
        assert_eq!(*launch.left(), Some(1));
        drop(launch.boarding());
        assert_eq!(*launch.left(), Some(0));
    }

    #[test]
    fn a_hop_waits_until_the_root_has_settled() {
        let launch = Launch::new();
        let waiter = {
            let boarding = launch.boarding();
            thread::spawn(move || {
                let started = Instant::now();
                boarding.wait_for_root();
                started.elapsed()
            })
        };
        thread::sleep(Duration::from_millis(40));
        launch.root_settled();
        let waited = waiter.join().unwrap();
        assert!(waited >= Duration::from_millis(30), "{waited:?}");
    }

    #[test]
    fn the_wait_ends_at_its_limit_when_the_size_never_comes() {
        let started = Instant::now();
        Launch::new().wait(Duration::from_millis(30));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
