pub mod bare_except;
pub mod no_color;
pub mod sys_exit;

use crate::check::Check;

/// Returns all Python source checks.
pub fn all_python_checks() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(bare_except::BareExceptCheck),
        Box::new(sys_exit::SysExitCheck),
        Box::new(no_color::NoColorPythonCheck),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_checks_registered() {
        let checks = all_python_checks();
        let ids: Vec<&str> = checks.iter().map(|c| c.id()).collect();
        assert!(ids.contains(&"code-bare-except"));
        assert!(ids.contains(&"p4-sys-exit"));
        assert!(ids.contains(&"p6-no-color"));
    }
}
