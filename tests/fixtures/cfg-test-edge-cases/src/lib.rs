// Fixture for the code-unwrap audit's cfg-gate exemption logic. Exercises the
// full parse -> walk -> AuditResult -> scorecard pipeline for the four cfg
// shapes that matter:
//
// 1. Production code (no gate): MUST be flagged.
// 2. `#[cfg(not(test))]` production-only items: MUST be flagged. This is the
//    regression guard for the PR #80 polarity fix.
// 3. `#[cfg(test)]`-gated functions: MUST be exempt.
// 4. `#[cfg(test)] mod`-gated items: MUST be exempt.
//
// The integration test in tests/integration.rs asserts exactly which lines
// surface in evidence; do not reorder items or add unrelated unwraps without
// updating the expected line numbers there.

fn maybe() -> Result<u32, ()> {
    Ok(42)
}

pub fn production_path() {
    let _ = maybe().unwrap();
}

#[cfg(not(test))]
pub fn production_only_path() {
    let _ = maybe().unwrap();
}

#[cfg(test)]
pub fn test_only_helper() {
    let _ = maybe().unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit() {
        let _ = maybe().unwrap();
    }
}
