mod about_long_about;
mod actionable_errors;
mod auto_verbosity;
mod bad_args;
mod bundle_install;
mod bundle_update;
mod color_flag;
mod consistent_envelope;
mod consistent_naming;
mod cursor_pagination;
mod defaults_in_help;
mod destructive_ops;
mod env_hints;
mod error_probe;
mod examples_subcommand;
mod flag_existence;
mod force_yes;
mod help;
mod install_all;
mod json_aliases;
mod json_error_output;
mod json_errors;
mod json_output;
mod limit_flag;
mod list_style;
mod more_formats;
mod no_color;
mod no_pager_behavioral;
mod non_interactive;
mod paired_examples;
mod quiet;
mod raw_flag;
mod read_write_distinction;
mod rich_tui;
mod schema_print;
mod secret_non_leaky_path;
mod sigpipe;
mod standard_names;
mod stdin_input;
mod structured_exit_codes;
mod subcommand_examples;
mod subcommand_help;
mod subcommand_operations;
mod timeout_behavioral;
mod verbose_flag;
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
        Box::new(flag_existence::FlagExistenceCheck),
        Box::new(env_hints::EnvHintsCheck),
        Box::new(no_pager_behavioral::NoPagerBehavioralCheck),
        Box::new(no_color::NoColorBehavioralCheck),
        Box::new(secret_non_leaky_path::SecretNonLeakyPathCheck),
        Box::new(schema_print::SchemaPrintCheck),
        Box::new(json_aliases::JsonAliasesCheck),
        Box::new(standard_names::StandardNamesCheck),
        Box::new(bundle_install::BundleInstallCheck),
        Box::new(install_all::InstallAllCheck),
        Box::new(bundle_update::BundleUpdateCheck),
        Box::new(raw_flag::RawFlagCheck),
        Box::new(more_formats::MoreFormatsCheck),
        Box::new(examples_subcommand::ExamplesSubcommandCheck),
        Box::new(color_flag::ColorFlagCheck),
        Box::new(verbose_flag::VerboseFlagCheck),
        Box::new(limit_flag::LimitFlagCheck),
        Box::new(cursor_pagination::CursorPaginationCheck),
        Box::new(defaults_in_help::DefaultsInHelpCheck),
        Box::new(rich_tui::RichTuiCheck),
        Box::new(about_long_about::AboutLongAboutCheck),
        Box::new(stdin_input::StdinInputCheck),
        Box::new(consistent_naming::ConsistentNamingCheck),
        Box::new(timeout_behavioral::TimeoutBehavioralCheck),
        Box::new(structured_exit_codes::StructuredExitCodesCheck),
        Box::new(actionable_errors::ActionableErrorsCheck),
        Box::new(json_errors::JsonErrorsCheck),
        Box::new(json_error_output::JsonErrorOutputCheck),
        Box::new(consistent_envelope::ConsistentEnvelopeCheck),
        Box::new(subcommand_examples::SubcommandExamplesCheck),
        Box::new(paired_examples::PairedExamplesCheck),
        Box::new(subcommand_operations::SubcommandOperationsCheck),
        Box::new(force_yes::ForceYesCheck),
        Box::new(read_write_distinction::ReadWriteDistinctionCheck),
        Box::new(auto_verbosity::AutoVerbosityCheck),
    ]
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;
    use std::sync::OnceLock;
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
            parsed_files: OnceLock::new(),
            help_output: OnceLock::new(),
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
            parsed_files: OnceLock::new(),
            help_output: OnceLock::new(),
        }
    }
}
