//! On Linux, anc adopts the processes its probes orphan.
//!
//! A probe can start a process that calls `setsid`, as Node's
//! `spawn(..., { detached: true })` does when an Electron launcher starts its
//! app. That process leaves the probe's process group, so the group kill
//! misses it, and once the probe exits it would be reparented to PID 1. As a
//! child subreaper anc is reparented those orphans instead, and after each
//! probe the runner kills and reaps every child of anc that is not a running
//! probe.

use std::sync::atomic::{AtomicBool, Ordering};

static ADOPTING: AtomicBool = AtomicBool::new(false);

/// Bounds the passes against a process that forks faster than they kill.
const MAX_PASSES: usize = 32;

pub(super) fn adopt() {
    // SAFETY: PR_SET_CHILD_SUBREAPER only sets an attribute of this process.
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } == 0 {
        ADOPTING.store(true, Ordering::Release);
    }
}

/// SIGKILL and reap every child of anc that `is_probe` does not claim. An
/// orphan's own children are orphaned to anc when it dies, so passes repeat
/// until one finds nothing to kill.
pub(super) fn reap(is_probe: impl Fn(libc::pid_t) -> bool) {
    if !ADOPTING.load(Ordering::Acquire) {
        return;
    }
    for _ in 0..MAX_PASSES {
        let orphans: Vec<libc::pid_t> = children()
            .into_iter()
            .filter(|&pid| !is_probe(pid))
            .collect();
        if orphans.is_empty() {
            return;
        }
        for &pid in &orphans {
            // SAFETY: kill only sends a signal.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
        for pid in orphans {
            // SAFETY: `pid` is a child of anc, and waitpid accepts a null
            // status pointer.
            unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) };
        }
    }
}

/// anc's children, read from every thread's list: a child is listed under
/// the thread that spawned or adopted it.
fn children() -> Vec<libc::pid_t> {
    let Ok(tasks) = std::fs::read_dir("/proc/self/task") else {
        return Vec::new();
    };
    tasks
        .flatten()
        .filter_map(|task| std::fs::read_to_string(task.path().join("children")).ok())
        .flat_map(|list| {
            list.split_whitespace()
                .filter_map(|pid| pid.parse().ok())
                .collect::<Vec<libc::pid_t>>()
        })
        .collect()
}
