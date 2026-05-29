pub mod enumerate_valid_set;
pub mod env_flags;
pub mod error_types;
pub mod exit_codes;
pub mod global_flags;
pub mod headless_auth;
pub mod naked_println;
pub mod no_color;
pub mod no_pager;
pub mod output_clamping;
pub mod output_module;
pub mod process_exit;
pub mod sigterm;
pub mod structured_output;
pub mod timeout_flag;
pub mod try_parse;
pub mod tty_detection;
pub mod unwrap;

use crate::audit::Audit;

/// Returns all Rust source audits.
pub fn all_rust_audits() -> Vec<Box<dyn Audit>> {
    vec![
        Box::new(unwrap::UnwrapAudit),
        Box::new(no_color::NoColorSourceAudit),
        Box::new(global_flags::GlobalFlagsAudit),
        Box::new(error_types::ErrorTypesAudit),
        Box::new(exit_codes::ExitCodesAudit),
        Box::new(process_exit::ProcessExitAudit),
        Box::new(try_parse::TryParseAudit),
        Box::new(env_flags::EnvFlagsAudit),
        Box::new(naked_println::NakedPrintlnAudit),
        Box::new(output_clamping::OutputClampingAudit),
        Box::new(headless_auth::HeadlessAuthAudit),
        Box::new(structured_output::StructuredOutputAudit),
        Box::new(no_pager::NoPagerAudit),
        Box::new(timeout_flag::TimeoutFlagAudit),
        Box::new(tty_detection::TtyDetectionAudit),
        Box::new(output_module::OutputModuleAudit),
        Box::new(enumerate_valid_set::EnumerateValidSetAudit),
        Box::new(sigterm::SigtermAudit),
    ]
}
