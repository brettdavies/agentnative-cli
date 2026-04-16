use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[cfg(unix)]
use anyhow::Context;
use anyhow::{Result, bail};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

/// Outcome classification for a binary execution.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // Crash is only constructed on unix (via ExitStatusExt::signal)
pub enum RunStatus {
    Ok,
    Timeout,
    Crash { signal: i32 },
    NotFound,
    PermissionDenied,
    Error(String),
}

/// Result of running a binary, including captured output.
#[derive(Debug, Clone)]
pub struct RunResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub status: RunStatus,
}

type CacheKey = (Vec<String>, Vec<(String, String)>);

/// Executes a binary with timeout, result caching, and partial-read support.
pub struct BinaryRunner {
    binary: PathBuf,
    timeout: Duration,
    cache: RefCell<HashMap<CacheKey, RunResult>>,
}

impl BinaryRunner {
    /// Create a new runner, validating the binary exists and is executable.
    pub fn new(binary: PathBuf, timeout: Duration) -> Result<Self> {
        if !binary.exists() {
            bail!("binary not found: {}", binary.display());
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&binary)
                .with_context(|| format!("cannot stat binary: {}", binary.display()))?;
            if meta.permissions().mode() & 0o111 == 0 {
                bail!("binary is not executable: {}", binary.display());
            }
        }

        Ok(Self {
            binary,
            timeout,
            cache: RefCell::new(HashMap::new()),
        })
    }

    /// Run the binary with the given args and env overrides.
    ///
    /// Results are cached by (args, env_overrides). `NO_COLOR=1` is always set.
    pub fn run(&self, args: &[&str], env_overrides: &[(&str, &str)]) -> RunResult {
        let cache_key: CacheKey = (
            args.iter().map(|s| (*s).to_owned()).collect(),
            env_overrides
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        );

        if let Some(cached) = self.cache.borrow().get(&cache_key) {
            return cached.clone();
        }

        let result = self.spawn_and_wait(args, env_overrides);

        self.cache.borrow_mut().insert(cache_key, result.clone());
        result
    }

    /// Run the binary but read only `read_bytes` from stdout, then drop the
    /// handle (triggering SIGPIPE on the child). Not cached.
    pub fn run_partial(&self, args: &[&str], read_bytes: usize) -> RunResult {
        let mut cmd = Command::new(&self.binary);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("NO_COLOR", "1")
            .env("AGENTNATIVE_CHECK", "1");

        let mut child = match Self::spawn_with_retry(&mut cmd) {
            Ok(c) => c,
            Err(e) => return Self::classify_spawn_error(e),
        };

        // Read only the requested number of bytes from stdout.
        let mut buf = vec![0u8; read_bytes];
        let stdout_handle = child.stdout.take();
        let bytes_read = match stdout_handle {
            Some(mut h) => {
                let mut total = 0;
                while total < read_bytes {
                    match h.read(&mut buf[total..]) {
                        Ok(0) => break,
                        Ok(n) => total += n,
                        Err(_) => break,
                    }
                }
                total
            }
            None => 0,
        };
        // stdout handle is dropped here, which may cause SIGPIPE.

        let stderr_output = match child.stderr.take() {
            Some(mut h) => {
                let mut s = String::new();
                let _ = h.read_to_string(&mut s);
                s
            }
            None => String::new(),
        };

        // Kill if still running, then wait.
        let _ = child.kill();
        let exit = child.wait().ok();

        let stdout_str = String::from_utf8_lossy(&buf[..bytes_read]).into_owned();

        RunResult {
            exit_code: exit.and_then(|s| s.code()),
            stdout: stdout_str,
            stderr: stderr_output,
            status: RunStatus::Ok,
        }
    }

    fn spawn_and_wait(&self, args: &[&str], env_overrides: &[(&str, &str)]) -> RunResult {
        let mut cmd = Command::new(&self.binary);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("NO_COLOR", "1")
            .env("AGENTNATIVE_CHECK", "1");

        for (k, v) in env_overrides {
            cmd.env(k, v);
        }

        let mut child = match Self::spawn_with_retry(&mut cmd) {
            Ok(c) => c,
            Err(e) => return Self::classify_spawn_error(e),
        };

        // Take stdout/stderr handles so reader threads own them.
        let stdout_handle = child.stdout.take();
        let stderr_handle = child.stderr.take();

        // Wrap child in Arc<Mutex> for shared access between timeout + main thread.
        let child = Arc::new(Mutex::new(child));

        // Reader threads.
        let stdout_thread = std::thread::spawn(move || {
            let mut out = String::new();
            if let Some(mut h) = stdout_handle {
                let _ = h.read_to_string(&mut out);
            }
            out
        });

        let stderr_thread = std::thread::spawn(move || {
            let mut out = String::new();
            if let Some(mut h) = stderr_handle {
                let _ = h.read_to_string(&mut out);
            }
            out
        });

        // Condvar-based timeout: the timeout thread sleeps until either the
        // child exits (signaled via condvar) or the deadline expires.
        // The poll loop uses try_wait so it never holds the child mutex for long.
        let done = Arc::new((Mutex::new(false), Condvar::new()));
        let done_for_timeout = Arc::clone(&done);
        let timed_out = Arc::new(Mutex::new(false));
        let timed_out_clone = Arc::clone(&timed_out);
        let timeout = self.timeout;
        let child_for_timeout = Arc::clone(&child);

        let timeout_thread = std::thread::spawn(move || {
            let (lock, cvar) = &*done_for_timeout;
            let guard = lock.lock().unwrap();
            // Check done flag first — if the child already exited before we
            // started, the condvar notification was already sent and would be lost.
            if *guard {
                return;
            }
            let (guard, timeout_result) = cvar.wait_timeout(guard, timeout).unwrap();
            if !*guard && timeout_result.timed_out() {
                *timed_out_clone.lock().unwrap() = true;
                if let Ok(mut c) = child_for_timeout.lock() {
                    let _ = c.kill();
                }
            }
        });

        // Poll for child exit with short sleeps — never holds child mutex long,
        // so the timeout thread can always acquire it to kill.
        let exit_status = loop {
            {
                let mut c = child.lock().unwrap();
                match c.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Ok(None) => {}
                    Err(_) => break None,
                }
            } // mutex released here
            if *timed_out.lock().unwrap() {
                let _ = child.lock().unwrap().wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };

        // Signal the timeout thread to wake up and exit immediately.
        {
            let (lock, cvar) = &*done;
            *lock.lock().unwrap() = true;
            cvar.notify_one();
        }
        timeout_thread.join().ok();

        let stdout = stdout_thread.join().unwrap_or_default();
        let stderr = stderr_thread.join().unwrap_or_default();

        let was_timeout = *timed_out.lock().unwrap();

        if was_timeout {
            return RunResult {
                exit_code: None,
                stdout,
                stderr,
                status: RunStatus::Timeout,
            };
        }

        Self::classify_exit(exit_status, stdout, stderr)
    }

    /// Spawn a command, retrying on ETXTBSY (errno 26) up to 50 times.
    /// ETXTBSY occurs when the executable was just written and the kernel
    /// hasn't fully released the write reference.
    fn spawn_with_retry(cmd: &mut Command) -> Result<std::process::Child, std::io::Error> {
        const MAX_RETRIES: u32 = 50;
        for attempt in 0..MAX_RETRIES {
            match cmd.spawn() {
                Ok(child) => return Ok(child),
                Err(e) if e.raw_os_error() == Some(26) && attempt < MAX_RETRIES - 1 => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!()
    }

    fn classify_spawn_error(e: std::io::Error) -> RunResult {
        let status = match e.kind() {
            std::io::ErrorKind::NotFound => RunStatus::NotFound,
            std::io::ErrorKind::PermissionDenied => RunStatus::PermissionDenied,
            _ => RunStatus::Error(e.to_string()),
        };
        RunResult {
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            status,
        }
    }

    #[cfg(unix)]
    fn classify_exit(
        exit_status: Option<std::process::ExitStatus>,
        stdout: String,
        stderr: String,
    ) -> RunResult {
        match exit_status {
            Some(status) => {
                if let Some(sig) = status.signal() {
                    RunResult {
                        exit_code: None,
                        stdout,
                        stderr,
                        status: RunStatus::Crash { signal: sig },
                    }
                } else {
                    RunResult {
                        exit_code: status.code(),
                        stdout,
                        stderr,
                        status: RunStatus::Ok,
                    }
                }
            }
            None => RunResult {
                exit_code: None,
                stdout,
                stderr,
                status: RunStatus::Error("failed to wait on child".into()),
            },
        }
    }

    #[cfg(not(unix))]
    fn classify_exit(
        exit_status: Option<std::process::ExitStatus>,
        stdout: String,
        stderr: String,
    ) -> RunResult {
        match exit_status {
            Some(status) => RunResult {
                exit_code: status.code(),
                stdout,
                stderr,
                status: RunStatus::Ok,
            },
            None => RunResult {
                exit_code: None,
                stdout,
                stderr,
                status: RunStatus::Error("failed to wait on child".into()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_echo() {
        let runner = BinaryRunner::new("/bin/echo".into(), Duration::from_secs(5))
            .expect("echo should exist");
        let result = runner.run(&["hello"], &[]);
        assert_eq!(result.status, RunStatus::Ok);
        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout.contains("hello"));
    }

    #[test]
    fn non_zero_exit() {
        let runner =
            BinaryRunner::new("/bin/sh".into(), Duration::from_secs(5)).expect("sh should exist");
        let result = runner.run(&["-c", "exit 42"], &[]);
        assert_eq!(result.status, RunStatus::Ok);
        assert_eq!(result.exit_code, Some(42));
    }

    #[test]
    fn cached_result() {
        let runner = BinaryRunner::new("/bin/echo".into(), Duration::from_secs(5))
            .expect("echo should exist");
        let r1 = runner.run(&["cache_test"], &[]);
        let r2 = runner.run(&["cache_test"], &[]);
        assert_eq!(r1.stdout, r2.stdout);
        assert_eq!(r1.exit_code, r2.exit_code);
        assert_eq!(r1.status, r2.status);
    }

    #[test]
    fn empty_output() {
        let runner =
            BinaryRunner::new("/bin/sh".into(), Duration::from_secs(5)).expect("sh should exist");
        let result = runner.run(&["-c", "true"], &[]);
        assert_eq!(result.status, RunStatus::Ok);
        assert!(result.stdout.is_empty());
    }

    #[test]
    fn partial_read() {
        let runner =
            BinaryRunner::new("/bin/sh".into(), Duration::from_secs(5)).expect("sh should exist");
        // Generate a long output, read only 5 bytes.
        let result = runner.run_partial(&["-c", "echo 'abcdefghijklmnopqrstuvwxyz'"], 5);
        assert_eq!(result.stdout.len(), 5);
        assert_eq!(&result.stdout, "abcde");
    }

    #[test]
    fn nonexistent_binary() {
        let err = BinaryRunner::new("/nonexistent/binary/xyz".into(), Duration::from_secs(5));
        assert!(err.is_err());
    }

    #[test]
    fn env_overrides_applied() {
        let runner =
            BinaryRunner::new("/bin/sh".into(), Duration::from_secs(5)).expect("sh should exist");
        let result = runner.run(&["-c", "echo $MY_TEST_VAR"], &[("MY_TEST_VAR", "42")]);
        assert_eq!(result.status, RunStatus::Ok);
        assert!(result.stdout.contains("42"));
    }

    #[test]
    #[ignore] // slow — spawns a sleep process
    fn timeout_kills_child() {
        let runner = BinaryRunner::new("/bin/sleep".into(), Duration::from_secs(1))
            .expect("sleep should exist");
        let result = runner.run(&["10"], &[]);
        assert_eq!(result.status, RunStatus::Timeout);
    }
}
