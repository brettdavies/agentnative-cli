//! Audits of this repository, run once per invocation and a few at a time.
//!
//! An audit of this repository is the heaviest spawn in the suite: its source
//! layer parses every file in the crate, about 25 CPU-seconds in a debug
//! build. libtest runs one test per core, and parallel agents run several test
//! runs on one machine at once, so unbounded these audits stack up by the
//! hundreds on a shared server. [`self_audit`] runs each distinct invocation
//! once per test binary and holds one of [`SLOTS`] machine-wide slots while it
//! runs.

use std::collections::HashMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::PathBuf;
use std::process::Output;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use assert_cmd::Command;

/// How many audits of this repository may run at once on one machine, across
/// every test binary and every test run. Four is a GitHub-hosted runner's
/// core count, so CI keeps its parallelism, and it holds this suite to a
/// quarter of a 16-core shared server.
pub const SLOTS: usize = 4;

/// One held slot: an exclusive lock on one slot file, released on drop.
pub struct AuditSlot {
    _lock: File,
}

/// Wait for a free slot and hold it. The slot files live in the system temp
/// directory, so every test process on the machine counts against the same
/// [`SLOTS`]; a lock dies with the process that held it, so a killed run
/// frees its slots.
pub fn audit_slot() -> AuditSlot {
    let dir = slot_dir();
    loop {
        for slot in 0..SLOTS {
            let path = dir.join(format!("slot-{slot}"));
            let file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(&path)
                .unwrap_or_else(|e| panic!("open audit slot {}: {e}", path.display()));
            match file.try_lock() {
                Ok(()) => return AuditSlot { _lock: file },
                Err(TryLockError::WouldBlock) => {}
                Err(TryLockError::Error(e)) => panic!("lock audit slot {}: {e}", path.display()),
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn slot_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("anc-test-audit-slots");
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|e| panic!("create audit slot directory {}: {e}", dir.display()));
    dir
}

/// `anc <args>` from the package root, which is this repository under
/// `cargo test`. Every test asking for the same arguments shares one run.
pub fn self_audit(args: &[&str]) -> Output {
    self_audit_with_env(args, &[])
}

/// [`self_audit`] with `envs` set on the spawned `anc`.
pub fn self_audit_with_env(args: &[&str], envs: &[(&str, &str)]) -> Output {
    type Key = (Vec<String>, Vec<(String, String)>);
    static RUNS: OnceLock<Mutex<HashMap<Key, Arc<OnceLock<Output>>>>> = OnceLock::new();

    let key = (
        args.iter().map(|arg| (*arg).to_owned()).collect(),
        envs.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    );
    let run = Arc::clone(
        RUNS.get_or_init(Mutex::default)
            .lock()
            .expect("self-audit registry")
            .entry(key)
            .or_default(),
    );
    run.get_or_init(|| {
        let _slot = audit_slot();
        Command::cargo_bin("anc")
            .expect("anc binary should exist")
            .env("AGENTNATIVE_HOME_CONFIG", super::empty_home_config())
            .envs(envs.iter().copied())
            .args(args)
            .output()
            .expect("spawn anc")
    })
    .clone()
}
