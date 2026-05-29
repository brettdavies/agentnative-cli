pub mod bare_except;
pub mod enumerate_valid_set;
pub mod no_color;
pub mod sigterm;
pub mod sys_exit;

use crate::audit::Audit;

/// Returns all Python source audits.
pub fn all_python_audits() -> Vec<Box<dyn Audit>> {
    vec![
        Box::new(bare_except::BareExceptAudit),
        Box::new(sys_exit::SysExitAudit),
        Box::new(no_color::NoColorPythonAudit),
        Box::new(enumerate_valid_set::EnumerateValidSetPythonAudit),
        Box::new(sigterm::SigtermPythonAudit),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_audits_registered() {
        let audits = all_python_audits();
        let ids: Vec<&str> = audits.iter().map(|c| c.id()).collect();
        assert!(ids.contains(&"code-bare-except"));
        assert!(ids.contains(&"p4-sys-exit"));
        assert!(ids.contains(&"p6-no-color"));
    }
}
