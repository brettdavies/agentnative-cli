use crate::check::Check;

/// Returns all Python source checks.
///
/// Currently empty — Python checks will be added in a future unit.
pub fn all_python_checks() -> Vec<Box<dyn Check>> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_checks_empty() {
        assert!(all_python_checks().is_empty());
    }
}
