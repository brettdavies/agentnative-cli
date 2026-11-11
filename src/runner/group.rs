//! One process group per probe, so what a probe starts ends with it.
//!
//! A tool's launcher can exit while the app it started keeps running, and a
//! killed shell leaves its children behind. Each probe leads a new process
//! group, and the runner kills the group once the probe has exited or timed
//! out, so whatever is still in it dies with the probe.
//!
//! A probe in its own group is outside the terminal's foreground group, so a
//! Ctrl-C no longer reaches it. anc forwards each signal that would have
//! reached it, and a terminate, to every live probe group, on Linux kills
//! what those probes moved out of their groups, then dies of the signal
//! itself.
//!
//! A group can only be forwarded to once it is registered, and a probe can be
//! running before `spawn` returns. So the handler is in place before the first
//! spawn, and a signal that lands during a spawn is held until the group is
//! registered. It is held in an atomic and not by blocking the signal, because
//! `Command` passes its thread's signal mask to the child, so a probe would start
//! with the signal blocked, and a mask covers only the calling thread.

use std::io;
use std::process::{Child, Command};

#[cfg(unix)]
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

/// The groups of the probes running now, `0` for a free slot. A signal
/// handler reads them, so they sit in a fixed array of atomics rather than
/// behind a lock. anc runs its probes one at a time, so 64 slots leave room
/// for parallel test threads; a probe that finds none free still dies with
/// its group, but is not sent a forwarded signal.
#[cfg(unix)]
static LIVE_GROUPS: [AtomicI32; 64] = [const { AtomicI32::new(0) }; 64];

/// How many spawns are under way. A signal that lands during one is held.
#[cfg(unix)]
static SPAWNING: AtomicUsize = AtomicUsize::new(0);

/// The signal held for the spawns under way, `0` for none.
#[cfg(unix)]
static HELD_SIGNAL: AtomicI32 = AtomicI32::new(0);

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

    /// Start a probe through `spawn`, on a command that went through
    /// [`ProbeGroup::isolate`], and register the group it leads. A forwarded
    /// signal that lands before the group is registered waits for it, then
    /// reaches the group.
    #[cfg(unix)]
    pub(super) fn spawn(spawn: impl FnOnce() -> io::Result<Child>) -> io::Result<(Child, Self)> {
        install_forwarding();
        let _held = HoldSignals::begin();
        let child = spawn()?;
        let group = Self::of(&child);
        Ok((child, group))
    }

    #[cfg(not(unix))]
    pub(super) fn spawn(spawn: impl FnOnce() -> io::Result<Child>) -> io::Result<(Child, Self)> {
        Ok((spawn()?, Self {}))
    }

    /// The group `child` leads.
    #[cfg(unix)]
    fn of(child: &Child) -> Self {
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

/// Holds forwarded signals from `begin` to its drop. The last spawn to finish
/// delivers what was held.
#[cfg(unix)]
struct HoldSignals;

#[cfg(unix)]
impl HoldSignals {
    fn begin() -> Self {
        SPAWNING.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

#[cfg(unix)]
impl Drop for HoldSignals {
    fn drop(&mut self) {
        if SPAWNING.fetch_sub(1, Ordering::SeqCst) == 1 {
            let held = HELD_SIGNAL.swap(0, Ordering::SeqCst);
            if held != 0 {
                end_probes_and_die(held);
            }
        }
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
    if SPAWNING.load(Ordering::SeqCst) > 0 {
        HELD_SIGNAL.store(signal, Ordering::SeqCst);
        // The last spawn can finish between that check and the store, and it
        // would then never see the signal. Whichever side swaps it out acts on
        // it, and SeqCst means at least one of them sees the other's write.
        if SPAWNING.load(Ordering::SeqCst) > 0 || HELD_SIGNAL.swap(0, Ordering::SeqCst) == 0 {
            return;
        }
    }
    end_probes_and_die(signal);
}

/// Called from the handler and from a thread finishing a spawn, so it makes
/// only async-signal-safe calls.
#[cfg(unix)]
fn end_probes_and_die(signal: libc::c_int) {
    for slot in &LIVE_GROUPS {
        let pgid = slot.load(Ordering::Acquire);
        if pgid > 0 {
            // `kill` on the negated group rather than `killpg`: only `kill`
            // is async-signal-safe.
            // SAFETY: kill only sends a signal.
            unsafe { libc::kill(-pgid, libc::SIGKILL) };
        }
    }
    #[cfg(target_os = "linux")]
    super::orphans::reap_for_signal();
    // SA_RESETHAND restored the default action, so the signal ends anc the
    // way it would have without the handler.
    // SAFETY: raise only sends a signal.
    unsafe { libc::raise(signal) };
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::*;

    /// Set in the process a test re-executes, to the file the probe appends
    /// its pids to.
    const PID_FILE_VAR: &str = "ANC_TEST_SPAWN_GAP_PID_FILE";

    fn recorded_pids(file: &Path) -> Vec<libc::pid_t> {
        std::fs::read_to_string(file)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.trim().parse().ok())
            .collect()
    }

    /// Start a hung probe that leaves a child behind, and signal this process
    /// once both are running and before the group is registered. The fixture
    /// reports its pids after it has exec'd, so the signal lands in the gap on
    /// every run. It only returns if the signal was lost.
    fn signal_this_process_between_spawn_and_registration(pid_file: &Path) {
        let probe = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/probe-leftovers/probe.sh");
        let mut cmd = Command::new(probe);
        ProbeGroup::isolate(&mut cmd)
            .env("ANC_TEST_PID_FILE", pid_file)
            .env("ANC_TEST_PROBE_HANG", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let started = ProbeGroup::spawn(|| {
            let child = cmd.spawn()?;
            let deadline = Instant::now() + Duration::from_secs(30);
            while recorded_pids(pid_file).len() < 2 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            // `raise` rather than `kill`: the handler runs on this thread before
            // it returns. A signal sent to the process can land on another
            // thread while this one goes on to register the group.
            // SAFETY: raise only sends a signal.
            unsafe { libc::raise(libc::SIGINT) };
            Ok(child)
        });
        std::thread::sleep(Duration::from_secs(5));
        drop(started);
    }

    /// Run `test` again in a process of its own, because the signal ends that
    /// process, and hold it to the contract: it dies of the signal, and nothing
    /// it started outlives it.
    fn assert_the_signal_ends_the_process_and_every_probe(test: &str) {
        let pids = tempfile::NamedTempFile::new().expect("temp file");
        let status = Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", test, "--test-threads=1", "--nocapture"])
            .env(PID_FILE_VAR, pids.path())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("test binary starts");

        let started = recorded_pids(pids.path());
        // SAFETY: signal 0 only checks that the process exists.
        let is_alive = |pid: libc::pid_t| unsafe { libc::kill(pid, 0) == 0 };
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut survivors = started.clone();
        while !survivors.is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            survivors.retain(|&pid| is_alive(pid));
        }
        for &pid in &survivors {
            // SAFETY: kill only sends a signal.
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }

        assert_eq!(started.len(), 2, "the probe did not start: {status:?}");
        assert_eq!(
            status.signal(),
            Some(libc::SIGINT),
            "the process exited with {status:?}"
        );
        assert!(
            survivors.is_empty(),
            "these probe processes outlived the signal: {survivors:?}"
        );
    }

    #[test]
    fn a_signal_during_the_first_spawn_reaches_the_probe_group() {
        match std::env::var_os(PID_FILE_VAR) {
            Some(file) => signal_this_process_between_spawn_and_registration(Path::new(&file)),
            None => assert_the_signal_ends_the_process_and_every_probe(
                "runner::group::tests::a_signal_during_the_first_spawn_reaches_the_probe_group",
            ),
        }
    }

    #[test]
    fn a_signal_during_a_later_spawn_reaches_the_probe_group() {
        match std::env::var_os(PID_FILE_VAR) {
            Some(file) => {
                let (_child, _group) =
                    ProbeGroup::spawn(|| Command::new("true").spawn()).expect("true starts");
                signal_this_process_between_spawn_and_registration(Path::new(&file));
            }
            None => assert_the_signal_ends_the_process_and_every_probe(
                "runner::group::tests::a_signal_during_a_later_spawn_reaches_the_probe_group",
            ),
        }
    }
}
