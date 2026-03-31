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
            runner: Some(BinaryRunner::new(PathBuf::from(binary), Duration::from_secs(5)).unwrap()),
            parsed_files: RefCell::new(HashMap::new()),
        }
    }

    /// Create a test project backed by `/bin/sh -c "<script>"`.
    ///
    /// This works by creating a temporary shell script file and pointing
    /// the runner at it.
    pub fn test_project_with_sh_script(script: &str) -> Project {
        use std::fs;
        use std::io::Write as _;
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let dir = std::env::temp_dir().join(format!(
            "agentnative-behavioral-tests-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();

        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let script_path = dir.join(format!("test-{id}.sh"));

        // Write to a temp file and rename for atomicity
        let tmp_path = script_path.with_extension("tmp");
        let mut f = fs::File::create(&tmp_path).unwrap();
        writeln!(f, "#!/bin/sh").unwrap();
        writeln!(f, "{script}").unwrap();
        f.sync_all().unwrap();
        drop(f);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        fs::rename(&tmp_path, &script_path).unwrap();

        Project {
            path: PathBuf::from("."),
            language: None,
            binary_paths: vec![script_path.clone()],
            manifest_path: None,
            runner: Some(BinaryRunner::new(script_path, Duration::from_secs(5)).unwrap()),
            parsed_files: RefCell::new(HashMap::new()),
        }
    }
}
