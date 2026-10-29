//! One process group per probe, so what a probe starts ends with it.
//!
//! A tool's launcher can exit while the app it started keeps running, and a
//! killed shell leaves its children behind. Each probe leads a new process
//! group, and the runner kills the group once the probe has exited or timed
//! out, so whatever is still in it dies with the probe.
//!
//! A probe in its own group is outside the terminal's foreground group, so a
//! Ctrl-C no longer reaches it. anc forwards each signal that would have
//! reached it, and a terminate, to every live probe group, then dies of the
//! signal itself.

use std::process::{Child, Command};

#[cfg(unix)]
use std::sync::atomic::{AtomicI32, Ordering};

/// The groups of the probes running now, `0` for a free slot. A signal
/// handler reads them, so they sit in a fixed array of atomics rather than
/// behind a lock. anc runs its probes one at a time, so 64 slots leave room
/// for parallel test threads; a probe that finds none free still dies with
/// its group, but is not sent a forwarded signal.
#[cfg(unix)]
static LIVE_GROUPS: [AtomicI32; 64] = [const { AtomicI32::new(0) }; 64];

#[cfg(unix)]
const FORWARDED: [libc::c_int; 4] = [libc::SIGHUP, libc::SIGINT, libc::SIGQUIT, libc::SIGTERM];

/// A running probe's process group. Dropping it kills whatever is left in
/// the group.
#[derive(Debug)]
pub(super) struct ProbeGroup {
    /// `None` once the group is killed.
    #[cfg(unix)]
    pgid: Option<libc::pid_t>,
    #[cfg(unix)]
    slot: Option<usize>,
}

impl ProbeGroup {
    pub(super) fn isolate(cmd: &mut Command) -> &mut Command {
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(cmd, 0);
        #[cfg(target_os = "linux")]
        die_with_anc(cmd);
        cmd
    }

    /// The group `child` leads, after [`ProbeGroup::isolate`] on its command.
    #[cfg(unix)]
    pub(super) fn of(child: &Child) -> Self {
        install_forwarding();
        let pgid = child.id() as libc::pid_t;
        let slot = LIVE_GROUPS.iter().position(|slot| {
            slot.compare_exchange(0, pgid, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
        });
        Self {
            pgid: Some(pgid),
            slot,
        }
    }

    #[cfg(not(unix))]
    pub(super) fn of(_child: &Child) -> Self {
        Self {}
    }

    /// SIGKILL every process left in the group, then, on Linux, every
    /// orphan anc adopted. Only once: after the kill the group's number can
    /// be reused, so a second kill or a forwarded signal could reach another
    /// group.
    pub(super) fn kill(&mut self) {
        #[cfg(unix)]
        if let Some(pgid) = self.pgid.take() {
            // SAFETY: killpg only sends a signal; a group with no members
            // left fails with ESRCH, which is the outcome wanted.
            unsafe { libc::killpg(pgid, libc::SIGKILL) };
            if let Some(slot) = self.slot.take() {
                LIVE_GROUPS[slot].store(0, Ordering::Release);
            }
            #[cfg(target_os = "linux")]
            super::orphans::reap(is_live_probe);
        }
    }
}

impl Drop for ProbeGroup {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Have the probe killed when the anc thread that spawned it exits. Nothing
/// is forwarded for a SIGKILL to anc, and the probe is no longer in anc's
/// process group, so without this a kill of anc's group would leave it
/// running. It covers only the probe itself, not what it starts.
#[cfg(target_os = "linux")]
fn die_with_anc(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    let anc = std::process::id() as libc::pid_t;
    // SAFETY: the closure runs between fork and exec and calls only prctl
    // and getppid, which are async-signal-safe.
    unsafe {
        cmd.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            // anc died before the prctl took effect.
            if libc::getppid() != anc {
                return Err(std::io::Error::from_raw_os_error(libc::ESRCH));
            }
            Ok(())
        });
    }
}

#[cfg(target_os = "linux")]
fn is_live_probe(pid: libc::pid_t) -> bool {
    LIVE_GROUPS
        .iter()
        .any(|slot| slot.load(Ordering::Acquire) == pid)
}

/// Route each [`FORWARDED`] signal whose action is still the default through
/// [`forward`]. A signal anc was started ignoring, as under `nohup` or as a
/// shell's background job, stays ignored.
#[cfg(unix)]
fn install_forwarding() {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        for signal in FORWARDED {
            // SAFETY: both structs are plain C data that zeroed memory
            // initializes, and `forward` only makes async-signal-safe calls.
            unsafe {
                let mut current: libc::sigaction = std::mem::zeroed();
                if libc::sigaction(signal, std::ptr::null(), &mut current) != 0
                    || !is_default(&current)
                {
                    continue;
                }
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = forward as extern "C" fn(libc::c_int) as libc::sighandler_t;
                action.sa_flags = libc::SA_RESETHAND;
                libc::sigemptyset(&mut action.sa_mask);
                libc::sigaction(signal, &action, std::ptr::null_mut());
            }
        }
    });
}

#[cfg(unix)]
fn is_default(action: &libc::sigaction) -> bool {
    action.sa_sigaction == libc::SIG_DFL
}

#[cfg(unix)]
extern "C" fn forward(signal: libc::c_int) {
    for slot in &LIVE_GROUPS {
        let pgid = slot.load(Ordering::Acquire);
        if pgid > 0 {
            // `kill` on the negated group rather than `killpg`: only `kill`
            // is async-signal-safe.
            // SAFETY: kill only sends a signal.
            unsafe { libc::kill(-pgid, libc::SIGKILL) };
        }
    }
    // SA_RESETHAND restored the default action, so the signal ends anc the
    // way it would have without the handler.
    // SAFETY: raise only sends a signal.
    unsafe { libc::raise(signal) };
}
