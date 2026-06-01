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

use crate::audit::Audit;

pub fn all_behavioral_audits() -> Vec<Box<dyn Audit>> {
    vec![
        Box::new(help::HelpAudit),
        Box::new(version::VersionAudit),
        Box::new(json_output::JsonOutputAudit),
        Box::new(bad_args::BadArgsAudit),
        Box::new(quiet::QuietAudit),
        Box::new(sigpipe::SigpipeAudit),
        Box::new(non_interactive::NonInteractiveAudit),
        Box::new(flag_existence::FlagExistenceAudit),
        Box::new(env_hints::EnvHintsAudit),
        Box::new(no_pager_behavioral::NoPagerBehavioralAudit),
        Box::new(no_color::NoColorBehavioralAudit),
        Box::new(secret_non_leaky_path::SecretNonLeakyPathAudit),
        Box::new(schema_print::SchemaPrintAudit),
        Box::new(json_aliases::JsonAliasesAudit),
        Box::new(standard_names::StandardNamesAudit),
        Box::new(bundle_install::BundleInstallAudit),
        Box::new(install_all::InstallAllAudit),
        Box::new(bundle_update::BundleUpdateAudit),
        Box::new(raw_flag::RawFlagAudit),
        Box::new(more_formats::MoreFormatsAudit),
        Box::new(examples_subcommand::ExamplesSubcommandAudit),
        Box::new(color_flag::ColorFlagAudit),
        Box::new(verbose_flag::VerboseFlagAudit),
        Box::new(limit_flag::LimitFlagAudit),
        Box::new(cursor_pagination::CursorPaginationAudit),
        Box::new(defaults_in_help::DefaultsInHelpAudit),
        Box::new(rich_tui::RichTuiAudit),
        Box::new(about_long_about::AboutLongAboutAudit),
        Box::new(stdin_input::StdinInputAudit),
        Box::new(consistent_naming::ConsistentNamingAudit),
        Box::new(timeout_behavioral::TimeoutBehavioralAudit),
        Box::new(structured_exit_codes::StructuredExitCodesAudit),
        Box::new(actionable_errors::ActionableErrorsAudit),
        Box::new(json_errors::JsonErrorsAudit),
        Box::new(json_error_output::JsonErrorOutputAudit),
        Box::new(consistent_envelope::ConsistentEnvelopeAudit),
        Box::new(subcommand_examples::SubcommandExamplesAudit),
        Box::new(paired_examples::PairedExamplesAudit),
        Box::new(subcommand_operations::SubcommandOperationsAudit),
        Box::new(force_yes::ForceYesAudit),
        Box::new(read_write_distinction::ReadWriteDistinctionAudit),
        Box::new(auto_verbosity::AutoVerbosityAudit),
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
