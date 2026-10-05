//! Where cargo writes the binaries of the Rust packages under an audit root.

use std::cell::OnceCell;
use std::ffi::{OsStr, OsString};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// How long `cargo metadata` may run before discovery falls back to
/// `<root>/target`. The same bound as a behavioral probe.
const METADATA_TIMEOUT: Duration = Duration::from_secs(5);

/// How often a running `cargo metadata` is checked for exit.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// The inputs that decide cargo's target directory: `CARGO_TARGET_DIR`, and
/// the cargo that reports what its config files and workspace decide.
#[derive(Debug, Clone)]
pub struct CargoTarget {
    /// `CARGO_TARGET_DIR`, resolved against the current directory.
    env: Option<PathBuf>,
    /// The program asked for `target_directory` when the variable is unset.
    cargo: OsString,
    timeout: Duration,
}

impl CargoTarget {
    /// The settings of the running process.
    pub fn from_env() -> Self {
        let cwd = std::env::current_dir().unwrap_or_default();
        Self {
            env: env_target_dir(std::env::var_os("CARGO_TARGET_DIR"), &cwd),
            cargo: OsString::from("cargo"),
            timeout: METADATA_TIMEOUT,
        }
    }

    /// As if `CARGO_TARGET_DIR` named `dir`.
    #[cfg(test)]
    pub fn at(dir: PathBuf) -> Self {
        Self {
            env: Some(dir),
            cargo: OsString::from("cargo"),
            timeout: METADATA_TIMEOUT,
        }
    }

    /// cargo's target directory for a build started in `root`:
    /// `CARGO_TARGET_DIR`, else the `target_directory` that `cargo metadata`
    /// reports there (which follows `build.target-dir` in cargo's config
    /// files and the enclosing workspace), else `<root>/target` when cargo
    /// cannot be run or does not answer.
    pub fn resolve(&self, root: &Path) -> PathBuf {
        if let Some(dir) = &self.env {
            return dir.clone();
        }
        metadata_target_dir(&self.cargo, root, self.timeout).unwrap_or_else(|| root.join("target"))
    }
}

/// The target directories the Rust packages under one audit root build
/// into. The root's is resolved once, on first use.
pub struct TargetDirs<'a> {
    cargo: &'a CargoTarget,
    root: &'a Path,
    root_dir: OnceCell<PathBuf>,
}

impl<'a> TargetDirs<'a> {
    pub fn new(cargo: &'a CargoTarget, root: &'a Path) -> Self {
        Self {
            cargo,
            root,
            root_dir: OnceCell::new(),
        }
    }

    /// Where the bins of the package at `package_root` are built, in search
    /// order: the package's own `target`, where cargo builds a package below
    /// the root that is not in the root's workspace, then the root's target
    /// directory. With `CARGO_TARGET_DIR` set, cargo builds every package
    /// there, so that is the only one.
    pub fn of(&self, package_root: &Path) -> Vec<PathBuf> {
        let root_dir = self.root_dir.get_or_init(|| self.cargo.resolve(self.root));
        let own = package_root.join("target");
        if self.cargo.env.is_some() || package_root == self.root || own == *root_dir {
            vec![root_dir.clone()]
        } else {
            vec![own, root_dir.clone()]
        }
    }
}

/// `CARGO_TARGET_DIR` as cargo reads it: a relative value is relative to the
/// current working directory, per
/// <https://doc.rust-lang.org/cargo/reference/environment-variables.html#environment-variables-cargo-reads>.
/// cargo rejects an empty value, so it builds nothing to look for.
fn env_target_dir(value: Option<OsString>, cwd: &Path) -> Option<PathBuf> {
    value.filter(|dir| !dir.is_empty()).map(|dir| cwd.join(dir))
}

/// The `target_directory` that `cargo metadata` reports when run in `dir`,
/// or `None` when cargo cannot start, fails, prints something unreadable,
/// or outlasts `timeout`.
fn metadata_target_dir(cargo: &OsStr, dir: &Path, timeout: Duration) -> Option<PathBuf> {
    let mut child = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(dir)
        // `CARGO_TARGET_DIR` reaches here only when empty, which cargo
        // rejects outright instead of answering from its config.
        .env_remove("CARGO_TARGET_DIR")
        // rustup otherwise installs a toolchain the project pins and this
        // machine lacks: a download that an audit never asked for.
        .env("RUSTUP_AUTO_INSTALL", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // A reader thread keeps a workspace's output from filling the pipe and
    // stalling cargo before it exits.
    let stdout = child.stdout.take();
    let reader = thread::spawn(move || {
        let mut out = Vec::new();
        stdout.map(|mut pipe| pipe.read_to_end(&mut out).map(|_| out))
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL_INTERVAL),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let out = reader.join().ok()??.ok()?;
    if !status.success() {
        return None;
    }
    let doc: serde_json::Value = serde_json::from_slice(&out).ok()?;
    doc.get("target_directory")?.as_str().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const CRATE: &str = "[package]\nname = \"tool\"\nversion = \"0.1.0\"\nedition = \"2024\"\n";

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, body).expect("write");
    }

    /// A crate in a fresh temp directory, by its canonical path: cargo
    /// reports paths under the directory it runs in, which the OS gives with
    /// symlinks resolved.
    fn crate_dir() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().canonicalize().expect("canonical tempdir");
        write(&root, "Cargo.toml", CRATE);
        write(&root, "src/main.rs", "fn main() {}\n");
        (tmp, root)
    }

    /// Settings with `CARGO_TARGET_DIR` unset, asking `cargo`.
    fn unset(cargo: impl Into<OsString>) -> CargoTarget {
        CargoTarget {
            env: None,
            cargo: cargo.into(),
            timeout: METADATA_TIMEOUT,
        }
    }

    #[test]
    fn an_absolute_env_value_is_the_target_directory() {
        let dir = std::env::temp_dir().join("elsewhere");

        assert_eq!(
            env_target_dir(Some(dir.clone().into_os_string()), Path::new("/work")),
            Some(dir)
        );
    }

    #[test]
    fn a_relative_env_value_is_relative_to_the_current_directory() {
        let cwd = std::env::temp_dir().join("work");

        assert_eq!(
            env_target_dir(Some(OsString::from("../shared/target")), &cwd),
            Some(cwd.join("../shared/target"))
        );
    }

    #[test]
    fn an_empty_or_missing_env_value_names_no_directory() {
        assert_eq!(env_target_dir(Some(OsString::new()), Path::new("/w")), None);
        assert_eq!(env_target_dir(None, Path::new("/w")), None);
    }

    #[test]
    fn the_env_value_wins_without_running_cargo() {
        let (_tmp, root) = crate_dir();
        let elsewhere = root.join("elsewhere");
        let settings = CargoTarget {
            env: Some(elsewhere.clone()),
            cargo: OsString::from("/nonexistent/cargo"),
            timeout: METADATA_TIMEOUT,
        };

        assert_eq!(settings.resolve(&root), elsewhere);
    }

    #[test]
    fn a_config_target_dir_is_where_cargo_metadata_says() {
        let (_tmp, root) = crate_dir();
        write(
            &root,
            ".cargo/config.toml",
            "[build]\ntarget-dir = \"out\"\n",
        );

        assert_eq!(unset("cargo").resolve(&root), root.join("out"));
    }

    #[test]
    fn a_workspace_member_builds_into_the_workspace_target_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ws = tmp.path().canonicalize().expect("canonical tempdir");
        write(
            &ws,
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/cli\"]\n",
        );
        write(&ws, "crates/cli/Cargo.toml", CRATE);
        write(&ws, "crates/cli/src/main.rs", "fn main() {}\n");
        let member = ws.join("crates/cli");
        let settings = unset("cargo");

        let resolved = settings.resolve(&member);

        assert_ne!(resolved, member.join("target"));
        assert_eq!(resolved, settings.resolve(&ws));
    }

    #[test]
    fn without_cargo_the_target_directory_is_the_roots() {
        let (_tmp, root) = crate_dir();

        assert_eq!(
            unset("/nonexistent/cargo").resolve(&root),
            root.join("target")
        );
    }

    #[test]
    fn a_failing_metadata_run_falls_back_to_the_roots() {
        let (_tmp, root) = crate_dir();
        write(&root, "Cargo.toml", "[package\nname = \"broken\"\n");

        assert_eq!(unset("cargo").resolve(&root), root.join("target"));
    }

    #[test]
    #[cfg(unix)]
    fn a_metadata_run_past_its_time_limit_falls_back_to_the_roots() {
        use std::os::unix::fs::PermissionsExt;
        let (_tmp, root) = crate_dir();
        let cargo = root.join("slow-cargo");
        fs::write(&cargo, "#!/bin/sh\nexec sleep 30\n").expect("write stand-in cargo");
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).expect("chmod");
        let settings = CargoTarget {
            env: None,
            cargo: cargo.into_os_string(),
            timeout: Duration::from_millis(200),
        };
        let started = Instant::now();

        assert_eq!(settings.resolve(&root), root.join("target"));
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn with_the_env_value_set_every_package_builds_there() {
        let root = Path::new("/work");
        let settings = CargoTarget::at(PathBuf::from("/shared/target"));
        let dirs = TargetDirs::new(&settings, root);

        assert_eq!(dirs.of(root), [PathBuf::from("/shared/target")]);
        assert_eq!(
            dirs.of(&root.join("cli")),
            [PathBuf::from("/shared/target")]
        );
    }

    #[test]
    fn without_it_a_package_below_the_root_also_searches_its_own_target() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().canonicalize().expect("canonical tempdir");
        write(&root, "cli/Cargo.toml", CRATE);
        let settings = unset("/nonexistent/cargo");
        let dirs = TargetDirs::new(&settings, &root);

        assert_eq!(dirs.of(&root), [root.join("target")]);
        assert_eq!(
            dirs.of(&root.join("cli")),
            [root.join("cli/target"), root.join("target")]
        );
    }
}
