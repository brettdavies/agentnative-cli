mod bad_args;
mod help;
mod json_output;
mod no_color;
mod non_interactive;
mod quiet;
mod sigpipe;
mod version;

use crate::check::Check;

pub fn all_behavioral_checks() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(help::HelpCheck),
        Box::new(version::VersionCheck),
        Box::new(json_output::JsonOutputCheck),
        Box::new(bad_args::BadArgsCheck),
        Box::new(quiet::QuietCheck),
        Box::new(sigpipe::SigpipeCheck),
        Box::new(non_interactive::NonInteractiveCheck),
        Box::new(no_color::NoColorBehavioralCheck),
    ]
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::Duration;

    use crate::project::Project;
    use crate::runner::BinaryRunner;

    /// Create a test project backed by the given binary path.
    pub fn test_project_with_runner(binary: &str) -> Project {
        Project {
            path: PathBuf::from("."),
            language: None,
            binary_paths: vec![PathBuf::from(binary)],
            manifest_path: None,
            runner: Some(
                BinaryRunner::new(PathBuf::from(binary), Duration::from_secs(5))
                    .expect("create test runner"),
            ),
            include_tests: false,
            parsed_files: RefCell::new(HashMap::new()),
        }
    }

    /// Create a test project backed by `/bin/sh -c "<script>"`.
    ///
    /// This works by creating a temporary shell script file and pointing
    /// the runner at it.
    pub fn test_project_with_sh_script(script: &str) -> Project {
        use std::fs;
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);

        // Use unique dir per call — counter + timestamp to avoid collisions
        let dir = std::env::temp_dir().join(format!(
            "agentnative-test-{}-{id}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after UNIX epoch")
                .as_nanos(),
        ));
        fs::create_dir_all(&dir).expect("create test dir");

        let script_path = dir.join("test.sh");
        let content = format!("#!/bin/sh\n{script}\n");

        // Write and set executable in one step to avoid ETXTBSY race between
        // fs::write close and set_permissions when tests run in parallel.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o755)
                .open(&script_path)
                .and_then(|mut f| {
                    use std::io::Write;
                    f.write_all(content.as_bytes())
                })
                .expect("write test script");
        }

        #[cfg(not(unix))]
        fs::write(&script_path, content).expect("write test script");

        Project {
            path: PathBuf::from("."),
            language: None,
            binary_paths: vec![script_path.clone()],
            manifest_path: None,
            runner: Some(
                BinaryRunner::new(script_path, Duration::from_secs(5)).expect("create test runner"),
            ),
            include_tests: false,
            parsed_files: RefCell::new(HashMap::new()),
        }
    }
}
