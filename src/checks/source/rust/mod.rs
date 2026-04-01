pub mod global_flags;
pub mod no_color;
pub mod unwrap;

use crate::check::Check;

/// Returns all Rust source checks.
pub fn all_rust_checks() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(unwrap::UnwrapCheck),
        Box::new(no_color::NoColorSourceCheck),
        Box::new(global_flags::GlobalFlagsCheck),
    ]
}
