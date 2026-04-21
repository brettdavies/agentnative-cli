//! Derived audience classifier. Reads a completed `results[]` vector and
//! labels the target as `agent_optimized`, `mixed`, or `human_primary` based
//! on `Warn` counts across a fixed set of 4 signal checks.
//!
//! The classifier is **informational, not authoritative**. It does not gate
//! scorecard totals, mutate per-check verdicts, or influence exit codes.
//! When any signal check is missing from the results — because it was
//! suppressed by an `--audit-profile`, Skipped by applicability, or never
//! ran at all — the classifier returns `None` rather than a partial-count
//! label. Partial signal is strictly weaker than per-check evidence.
//!
//! Per CEO review Finding #3 (2026-04-20): a tool whose label disagrees
//! with intuition is fixed by adding an `audit_profile` in the registry or
//! introducing a new MUST in the spec — never by adjusting the classifier
//! rules. Keep this file read-only over `results`.
//!
//! Labels serialize as snake_case to match the existing enum conventions
//! (`CheckGroup`, `CheckLayer`, `Confidence`).

use crate::types::{CheckResult, CheckStatus};

/// The four behavioral checks whose verdicts define a CLI's audience.
/// Ordered by principle for grep-friendliness; the classifier treats them
/// as an unordered set. A rename in any of these check IDs MUST be paired
/// with a rename here — the inline drift test catches the gap at build time.
pub const SIGNAL_CHECK_IDS: [&str; 4] = [
    "p1-non-interactive",
    "p2-json-output",
    "p7-quiet",
    "p6-no-color-behavioral",
];

/// Classify the target's audience from a completed results vector.
///
/// Returns `None` when fewer than 4 of the signal checks appear in
/// `results` — partial signal cannot produce an honest label. Counts
/// only `CheckStatus::Warn`; every other status (including `Skip` and
/// `Error`) is treated as not-a-Warn, so the denominator stays at 4
/// but the classifier errs toward `agent_optimized` when signal is
/// ambiguous rather than punishing it.
pub fn classify(results: &[CheckResult]) -> Option<String> {
    let mut warns = 0usize;
    let mut matched = 0usize;

    for signal_id in SIGNAL_CHECK_IDS {
        let Some(r) = results.iter().find(|r| r.id == signal_id) else {
            continue;
        };
        matched += 1;
        if matches!(r.status, CheckStatus::Warn(_)) {
            warns += 1;
        }
    }

    if matched < SIGNAL_CHECK_IDS.len() {
        return None;
    }

    Some(
        match warns {
            0..=1 => "agent_optimized",
            2 => "mixed",
            _ => "human_primary",
        }
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principles::registry;
    use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

    fn signal_result(id: &str, status: CheckStatus) -> CheckResult {
        CheckResult {
            id: id.to_string(),
            label: format!("Signal: {id}"),
            group: CheckGroup::P1,
            layer: CheckLayer::Behavioral,
            status,
            confidence: Confidence::High,
        }
    }

    fn all_pass() -> Vec<CheckResult> {
        SIGNAL_CHECK_IDS
            .iter()
            .map(|id| signal_result(id, CheckStatus::Pass))
            .collect()
    }

    #[test]
    fn four_passes_yields_agent_optimized() {
        assert_eq!(classify(&all_pass()).as_deref(), Some("agent_optimized"));
    }

    #[test]
    fn four_warns_yields_human_primary() {
        let results: Vec<_> = SIGNAL_CHECK_IDS
            .iter()
            .map(|id| signal_result(id, CheckStatus::Warn("missing".into())))
            .collect();
        assert_eq!(classify(&results).as_deref(), Some("human_primary"));
    }

    #[test]
    fn two_warns_yields_mixed() {
        let mut results = all_pass();
        results[0].status = CheckStatus::Warn("x".into());
        results[1].status = CheckStatus::Warn("y".into());
        assert_eq!(classify(&results).as_deref(), Some("mixed"));
    }

    #[test]
    fn one_warn_yields_agent_optimized() {
        let mut results = all_pass();
        results[2].status = CheckStatus::Warn("soft".into());
        assert_eq!(classify(&results).as_deref(), Some("agent_optimized"));
    }

    #[test]
    fn three_warns_yields_human_primary() {
        let mut results = all_pass();
        results[0].status = CheckStatus::Warn("a".into());
        results[1].status = CheckStatus::Warn("b".into());
        results[2].status = CheckStatus::Warn("c".into());
        assert_eq!(classify(&results).as_deref(), Some("human_primary"));
    }

    #[test]
    fn missing_one_signal_returns_none() {
        // Drop the last signal check entirely — classifier must refuse.
        let results: Vec<_> = SIGNAL_CHECK_IDS[..3]
            .iter()
            .map(|id| signal_result(id, CheckStatus::Pass))
            .collect();
        assert!(classify(&results).is_none());
    }

    #[test]
    fn skipped_signal_counts_as_not_warn() {
        // Skip is a legitimate outcome (e.g., target has no flags); it's not
        // a Warn, so it doesn't push toward human_primary, and the signal
        // still counts toward the denominator — all 4 present.
        let mut results = all_pass();
        results[3].status = CheckStatus::Skip("no flags".into());
        assert_eq!(classify(&results).as_deref(), Some("agent_optimized"));
    }

    #[test]
    fn errored_signal_counts_as_not_warn() {
        let mut results = all_pass();
        results[0].status = CheckStatus::Error("runner crashed".into());
        assert_eq!(classify(&results).as_deref(), Some("agent_optimized"));
    }

    #[test]
    fn non_signal_results_are_ignored() {
        // Add a non-signal Warn — must not influence the verdict.
        let mut results = all_pass();
        results.push(signal_result("p6-sigpipe", CheckStatus::Warn("x".into())));
        results.push(signal_result("p4-bad-args", CheckStatus::Warn("y".into())));
        assert_eq!(classify(&results).as_deref(), Some("agent_optimized"));
    }

    #[test]
    fn empty_results_returns_none() {
        assert!(classify(&[]).is_none());
    }

    /// Drift guard: every signal ID must be a registered check requirement
    /// so a rename on one side of the house surfaces the break on the other.
    /// Signal IDs are *check* IDs, not requirement IDs — but each signal
    /// check covers at least one requirement, and a rename that breaks the
    /// signal set also breaks the classifier. The cheap drift test is to
    /// confirm the check exists in the behavioral catalog at test time;
    /// the authoritative guard is the dangling_cover_ids test already in
    /// `matrix.rs`, which runs over the full catalog.
    #[test]
    fn signal_check_ids_are_present_in_behavioral_catalog() {
        use crate::check::Check;
        use crate::checks::behavioral::all_behavioral_checks;

        let behavioral: Vec<Box<dyn Check>> = all_behavioral_checks();
        let behavioral_ids: Vec<&str> = behavioral.iter().map(|c| c.id()).collect();

        for signal_id in SIGNAL_CHECK_IDS {
            assert!(
                behavioral_ids.contains(&signal_id),
                "signal check `{signal_id}` is missing from the behavioral catalog — \
                 a rename or removal broke the audience classifier. Update \
                 `SIGNAL_CHECK_IDS` in `src/scorecard/audience.rs` to match.",
            );
        }
    }

    /// Sanity: the signal check IDs' principle prefixes must exist in the
    /// requirement registry. Guards against a principle-level rename
    /// slipping through even when the check ID itself stays the same.
    #[test]
    fn signal_ids_reference_known_principles() {
        for signal_id in SIGNAL_CHECK_IDS {
            // Extract `pN-` prefix and confirm at least one requirement in
            // the registry carries the same principle number.
            let prefix = signal_id
                .split_once('-')
                .map(|(p, _)| p)
                .expect("signal id has pN- prefix");
            let hits = registry::REQUIREMENTS
                .iter()
                .filter(|r| format!("p{}", r.principle) == prefix)
                .count();
            assert!(
                hits > 0,
                "signal check `{signal_id}` has principle prefix `{prefix}` \
                 but no registry requirements match — classifier will \
                 silently drift from spec. Fix `SIGNAL_CHECK_IDS`.",
            );
        }
    }
}
