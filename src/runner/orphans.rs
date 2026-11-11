//! On Linux, anc adopts the processes its probes orphan.
//!
//! A probe can start a process that calls `setsid`, as Node's
//! `spawn(..., { detached: true })` does when an Electron launcher starts its
//! app. That process leaves the probe's process group, so the group kill
//! misses it, and once the probe exits it would be reparented to PID 1. As a
//! child subreaper anc is reparented those orphans instead, and after each
//! probe the runner kills and reaps every child of anc that is not a running
//! probe.

use std::ffi::{CStr, CString};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static ADOPTING: AtomicBool = AtomicBool::new(false);

/// The main thread's children list, which orphans are added to: the kernel
/// gives them to a subreaper's first live thread. Built in [`adopt`], since
/// a signal handler cannot format a path.
static MAIN_CHILDREN: OnceLock<CString> = OnceLock::new();

/// Bounds the passes against a process that forks faster than they kill.
const MAX_PASSES: usize = 32;

pub(super) fn adopt() {
    // SAFETY: PR_SET_CHILD_SUBREAPER only sets an attribute of this process.
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } == 0 {
        let main = format!("/proc/self/task/{}/children", std::process::id());
        if let Ok(main) = CString::new(main) {
            let _ = MAIN_CHILDREN.set(main);
        }
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

/// SIGKILL and reap every child of anc, its probes included, for a signal
/// that is ending anc. What a probe moved out of its group is adopted by anc
/// as the probe dies, and would be reparented to PID 1 once anc dies. Runs
/// in a signal handler, so it reads the list with `open` and `read` into a
/// fixed buffer and allocates nothing.
pub(super) fn reap_for_signal() {
    let Some(path) = MAIN_CHILDREN.get() else {
        return;
    };
    for _ in 0..MAX_PASSES {
        let mut list = [0u8; 4096];
        let read = read_list(path, &mut list);
        let mut found = false;
        for pid in pids(&list[..read]) {
            found = true;
            // SAFETY: kill only sends a signal, and `pid` is positive.
            unsafe { libc::kill(pid, libc::SIGKILL) };
            // SAFETY: `pid` is a child of anc, and waitpid accepts a null
            // status pointer.
            unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) };
        }
        if !found {
            return;
        }
    }
}

/// Reads the file at `path` into `buf` and returns how many bytes it read, 0
/// when it cannot be opened.
fn read_list(path: &CStr, buf: &mut [u8]) -> usize {
    // SAFETY: `path` is NUL-terminated.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC) };
    if fd < 0 {
        return 0;
    }
    let mut total = 0;
    while total < buf.len() {
        // SAFETY: the pointer and length stay inside `buf`.
        let n = unsafe { libc::read(fd, buf[total..].as_mut_ptr().cast(), buf.len() - total) };
        if n <= 0 {
            break;
        }
        total += n as usize;
    }
    // SAFETY: `fd` is open and owned here.
    unsafe { libc::close(fd) };
    total
}

/// The pids in a children list, each followed by a space. A last token with
/// no space after it was cut short by the buffer, so it is dropped rather
/// than read as a different pid.
fn pids(list: &[u8]) -> impl Iterator<Item = libc::pid_t> + '_ {
    let complete = list.iter().filter(|&&b| b == b' ').count();
    list.split(|&b| b == b' ')
        .take(complete)
        .filter_map(|token| {
            let mut pid: libc::pid_t = 0;
            for &b in token {
                if !b.is_ascii_digit() {
                    return None;
                }
                pid = pid
                    .checked_mul(10)?
                    .checked_add(libc::pid_t::from(b - b'0'))?;
            }
            (pid > 0).then_some(pid)
        })
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

#[cfg(test)]
mod tests {
    use super::pids;

    #[test]
    fn a_children_list_cut_short_drops_its_last_pid() {
        assert_eq!(pids(b"12 345 67").collect::<Vec<_>>(), vec![12, 345]);
        assert_eq!(pids(b"12 345 ").collect::<Vec<_>>(), vec![12, 345]);
        assert_eq!(pids(b"").count(), 0);
    }
}
