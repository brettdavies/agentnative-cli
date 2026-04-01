pub mod env_flags;
pub mod error_types;
pub mod exit_codes;
pub mod global_flags;
pub mod headless_auth;
pub mod naked_println;
pub mod no_color;
pub mod no_pager;
pub mod output_clamping;
pub mod process_exit;
pub mod structured_output;
pub mod timeout_flag;
pub mod try_parse;
pub mod tty_detection;
pub mod unwrap;

use crate::check::Check;

/// Returns all Rust source checks.
pub fn all_rust_checks() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(unwrap::UnwrapCheck),
        Box::new(no_color::NoColorSourceCheck),
        Box::new(global_flags::GlobalFlagsCheck),
        Box::new(error_types::ErrorTypesCheck),
        Box::new(exit_codes::ExitCodesCheck),
        Box::new(process_exit::ProcessExitCheck),
        Box::new(try_parse::TryParseCheck),
        Box::new(env_flags::EnvFlagsCheck),
        Box::new(naked_println::NakedPrintlnCheck),
        Box::new(output_clamping::OutputClampingCheck),
        Box::new(headless_auth::HeadlessAuthCheck),
        Box::new(structured_output::StructuredOutputCheck),
        Box::new(no_pager::NoPagerCheck),
        Box::new(timeout_flag::TimeoutFlagCheck),
        Box::new(tty_detection::TtyDetectionCheck),
    ]
}
