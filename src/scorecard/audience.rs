//! Derived audience classifier. Reads a completed `results[]` vector and
//! labels the target as `agent-optimized`, `mixed`, or `human-primary` based
//! on `Warn` counts across a fixed set of 4 signal audits.
//!
//! The classifier is **informational, not authoritative**. It does not gate
//! scorecard totals, mutate per-audit verdicts, or influence exit codes.
//! When any signal audit is missing from the results — because it was
//! suppressed by an `--audit-profile`, Skipped by applicability, or never
//! ran at all — the classifier returns `None` rather than a partial-count
//! label. Partial signal is strictly weaker than per-audit evidence.
//!
//! Per CEO review Finding #3 (2026-04-20): a tool whose label disagrees
//! with intuition is fixed by adding an `audit_profile` in the registry or
//! introducing a new MUST in the spec — never by adjusting the classifier
//! rules. Keep this file read-only over `results`.
//!
//! Labels serialize as kebab-case (`agent-optimized`, `mixed`,
//! `human-primary`) to match `audit_profile`'s kebab-case values within
//! the same JSON document. The `AuditGroup` / `AuditLayer` / `Confidence`
//! enums stay snake_case as type-system identifiers — those are a
//! different contract (one per `results[]` row) with broader consumer
//! history.

use crate::types::{AuditResult, AuditStatus};

/// The four behavioral audits whose verdicts define a CLI's audience.
/// Ordered by principle for grep-friendliness; the classifier treats them
/// as an unordered set. A rename in any of these audit IDs MUST be paired
/// with a rename here — the inline drift test catches the gap at build time.
pub const SIGNAL_AUDIT_IDS: [&str; 4] = [
    "p1-non-interactive",
    "p2-json-output",
    "p7-quiet",
    "p6-no-color-behavioral",
];

/// Classify the target's audience from a completed results vector.
///
/// Returns `None` when fewer than 4 of the signal audits appear in
/// `results` — partial signal cannot produce an honest label. Counts
/// only `AuditStatus::Warn`; every other status (including `Skip` and
/// `Error`) is treated as not-a-Warn, so the denominator stays at 4
/// but the classifier errs toward `agent-optimized` when signal is
/// ambiguous rather than punishing it.
pub fn classify(results: &[AuditResult]) -> Option<String> {
    let mut warns = 0usize;
    let mut matched = 0usize;

    for signal_id in SIGNAL_AUDIT_IDS {
        // Duplicate signal IDs in `results[]` are a registry bug — the
        // audit catalog should guarantee uniqueness. `find()` takes the
        // first match silently, which would mask a regression. Trip in
        // debug builds so it surfaces in `cargo test` instead of shipping.
        debug_assert!(
            results.iter().filter(|r| r.id == signal_id).count() <= 1,
            "duplicate signal audit ID in results[]: {signal_id}. \
             Every behavioral audit ID must be unique across the catalog; \
             classify() uses iter().find() and would silently ignore the duplicate.",
        );
        let Some(r) = results.iter().find(|r| r.id == signal_id) else {
            continue;
        };
        // A Skip emitted by `--audit-profile` suppression is *not* signal —
        // per R2, any signal audit that didn't run produces `audience:
        // null`. Organic Skips (e.g., "no flags exposed") still count
        // toward the denominator because the audit actually executed.
        if is_audit_profile_suppression(&r.status) {
            continue;
        }
        matched += 1;
        if matches!(r.status, AuditStatus::Warn(_)) {
            warns += 1;
        }
    }

    if matched < SIGNAL_AUDIT_IDS.len() {
        return None;
    }

    Some(
        match warns {
            0..=1 => "agent-optimized",
            2 => "mixed",
            _ => "human-primary",
        }
        .to_string(),
    )
}

/// Diagnose *why* [`classify`] returned `None`. When the classifier
/// withholds a label, callers (notably the scorecard emitter) can
/// surface `audience_reason` so agents and UI don't have to rerun the
/// logic to answer "why is this null".
///
/// - `None` — audience has a concrete label; there's no gap to explain.
/// - `Some("suppressed")` — at least one signal audit was skipped by
///   `--audit-profile`. Caller chose to mask the signal; the right fix
///   is to pick a different profile or live with the null.
/// - `Some("insufficient_signal")` — at least one signal audit didn't
///   run at all (e.g., `--source` mode, missing runner, or an unsupported
///   target). Caller can't fix this with a flag.
///
/// Suppression dominates when both conditions apply — the caller-chosen
/// mask is the more actionable signal than "the audit wasn't produced".
pub fn classify_reason(results: &[AuditResult]) -> Option<&'static str> {
    let mut any_suppressed = false;
    let mut any_missing = false;
    for signal_id in SIGNAL_AUDIT_IDS {
        match results.iter().find(|r| r.id == signal_id) {
            None => any_missing = true,
            Some(r) if is_audit_profile_suppression(&r.status) => any_suppressed = true,
            Some(_) => {}
        }
    }
    if !any_suppressed && !any_missing {
        return None;
    }
    if any_suppressed {
        Some("suppressed")
    } else {
        Some("insufficient_signal")
    }
}

/// Evidence-string sniff for audit_profile suppression. The prefix is
/// produced by the main audit-execution loop and consumed here and by
/// `build_coverage_summary`. The single source of truth is
/// `registry::SUPPRESSION_EVIDENCE_PREFIX`; a rename there propagates
/// without silent drift between producer and consumers.
pub(crate) fn is_audit_profile_suppression(status: &AuditStatus) -> bool {
    matches!(
        status,
        AuditStatus::Skip(e) if e.starts_with(crate::principles::registry::SUPPRESSION_EVIDENCE_PREFIX)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principles::registry;
    use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

    fn signal_result(id: &str, status: AuditStatus) -> AuditResult {
        AuditResult {
            id: id.to_string(),
            label: format!("Signal: {id}"),
            group: AuditGroup::P1,
            layer: AuditLayer::Behavioral,
            status,
            confidence: Confidence::High,
        }
    }

    fn all_pass() -> Vec<AuditResult> {
        SIGNAL_AUDIT_IDS
            .iter()
            .map(|id| signal_result(id, AuditStatus::Pass))
            .collect()
    }

    #[test]
    fn four_passes_yields_agent_optimized() {
        assert_eq!(classify(&all_pass()).as_deref(), Some("agent-optimized"));
    }

    #[test]
    fn four_warns_yields_human_primary() {
        let results: Vec<_> = SIGNAL_AUDIT_IDS
            .iter()
            .map(|id| signal_result(id, AuditStatus::Warn("missing".into())))
            .collect();
        assert_eq!(classify(&results).as_deref(), Some("human-primary"));
    }

    #[test]
    fn two_warns_yields_mixed() {
        let mut results = all_pass();
        results[0].status = AuditStatus::Warn("x".into());
        results[1].status = AuditStatus::Warn("y".into());
        assert_eq!(classify(&results).as_deref(), Some("mixed"));
    }

    #[test]
    fn one_warn_yields_agent_optimized() {
        let mut results = all_pass();
        results[2].status = AuditStatus::Warn("soft".into());
        assert_eq!(classify(&results).as_deref(), Some("agent-optimized"));
    }

    #[test]
    fn three_warns_yields_human_primary() {
        let mut results = all_pass();
        results[0].status = AuditStatus::Warn("a".into());
        results[1].status = AuditStatus::Warn("b".into());
        results[2].status = AuditStatus::Warn("c".into());
        assert_eq!(classify(&results).as_deref(), Some("human-primary"));
    }

    #[test]
    fn missing_one_signal_returns_none() {
        // Drop the last signal audit entirely — classifier must refuse.
        let results: Vec<_> = SIGNAL_AUDIT_IDS[..3]
            .iter()
            .map(|id| signal_result(id, AuditStatus::Pass))
            .collect();
        assert!(classify(&results).is_none());
    }

    #[test]
    fn organic_skipped_signal_counts_as_not_warn() {
        // An organic Skip (e.g., target has no flags) is a legitimate
        // outcome — the audit ran and decided it doesn't apply. It's not
        // a Warn, so it doesn't push toward human-primary, and the signal
        // still counts toward the denominator — all 4 present.
        let mut results = all_pass();
        results[3].status = AuditStatus::Skip("no flags".into());
        assert_eq!(classify(&results).as_deref(), Some("agent-optimized"));
    }

    #[test]
    fn audit_profile_suppressed_signal_drops_denominator() {
        // When --audit-profile suppresses a signal audit, the evidence
        // prefix tells us the audit did NOT run — per R2, audience must
        // be null rather than a partial-count verdict.
        let mut results = all_pass();
        results[0].status = AuditStatus::Skip(format!(
            "{}human-tui",
            crate::principles::registry::SUPPRESSION_EVIDENCE_PREFIX
        ));
        assert!(
            classify(&results).is_none(),
            "audit_profile-suppressed signal should drop denominator and force None",
        );
    }

    #[test]
    fn errored_signal_counts_as_not_warn() {
        let mut results = all_pass();
        results[0].status = AuditStatus::Error("runner crashed".into());
        assert_eq!(classify(&results).as_deref(), Some("agent-optimized"));
    }

    #[test]
    fn classify_reason_none_when_audience_has_label() {
        let results = all_pass();
        assert!(classify(&results).is_some());
        assert_eq!(classify_reason(&results), None);
    }

    #[test]
    fn classify_reason_suppressed_when_signal_audit_profile_suppressed() {
        let mut results = all_pass();
        results[0].status = AuditStatus::Skip(format!(
            "{}human-tui",
            crate::principles::registry::SUPPRESSION_EVIDENCE_PREFIX
        ));
        assert!(classify(&results).is_none());
        assert_eq!(classify_reason(&results), Some("suppressed"));
    }

    #[test]
    fn classify_reason_insufficient_when_signal_missing() {
        // Source-only-style run: 0 signal audits present.
        let results: Vec<AuditResult> = Vec::new();
        assert!(classify(&results).is_none());
        assert_eq!(classify_reason(&results), Some("insufficient_signal"));
    }

    #[test]
    fn classify_reason_suppressed_dominates_missing() {
        // One signal missing + one signal suppressed → reason is
        // `suppressed` (caller-chosen mask is more actionable).
        let results: Vec<AuditResult> = SIGNAL_AUDIT_IDS
            .iter()
            .take(3) // drop the 4th entirely
            .enumerate()
            .map(|(i, id)| {
                let status = if i == 0 {
                    AuditStatus::Skip(format!(
                        "{}human-tui",
                        crate::principles::registry::SUPPRESSION_EVIDENCE_PREFIX
                    ))
                } else {
                    AuditStatus::Pass
                };
                signal_result(id, status)
            })
            .collect();
        assert_eq!(classify_reason(&results), Some("suppressed"));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "duplicate signal audit ID")]
    fn duplicate_signal_in_results_trips_debug_assert() {
        // Two results with the same signal ID is a registry drift hazard —
        // the catalog should guarantee uniqueness. classify() must trip
        // in debug builds rather than silently picking the first entry.
        //
        // Gated on `debug_assertions` because `cargo test --release` compiles
        // `debug_assert!` to a no-op, so the panic this test asserts cannot
        // fire there. The drift hazard the test guards is a development-time
        // concern (refactors that introduce duplicate IDs); the release-build
        // surface is the registry uniqueness contract enforced by the
        // catalog assembly, which has its own dedicated tests.
        let mut results = all_pass();
        // Duplicate p1-non-interactive.
        results.push(signal_result(SIGNAL_AUDIT_IDS[0], AuditStatus::Pass));
        classify(&results);
    }

    #[test]
    fn non_signal_results_are_ignored() {
        // Add a non-signal Warn — must not influence the verdict.
        let mut results = all_pass();
        results.push(signal_result("p6-sigpipe", AuditStatus::Warn("x".into())));
        results.push(signal_result("p4-bad-args", AuditStatus::Warn("y".into())));
        assert_eq!(classify(&results).as_deref(), Some("agent-optimized"));
    }

    #[test]
    fn empty_results_returns_none() {
        assert!(classify(&[]).is_none());
    }

    /// Drift guard: every signal ID must be a registered audit requirement
    /// so a rename on one side of the house surfaces the break on the other.
    /// Signal IDs are *audit* IDs, not requirement IDs — but each signal
    /// audit covers at least one requirement, and a rename that breaks the
    /// signal set also breaks the classifier. The cheap drift test is to
    /// confirm the audit exists in the behavioral catalog at test time;
    /// the authoritative guard is the dangling_cover_ids test already in
    /// `matrix.rs`, which runs over the full catalog.
    #[test]
    fn signal_audit_ids_are_present_in_behavioral_catalog() {
        use crate::audit::Audit;
        use crate::audits::behavioral::all_behavioral_audits;

        let behavioral: Vec<Box<dyn Audit>> = all_behavioral_audits();
        let behavioral_ids: Vec<&str> = behavioral.iter().map(|c| c.id()).collect();

        for signal_id in SIGNAL_AUDIT_IDS {
            assert!(
                behavioral_ids.contains(&signal_id),
                "signal audit `{signal_id}` is missing from the behavioral catalog — \
                 a rename or removal broke the audience classifier. Update \
                 `SIGNAL_AUDIT_IDS` in `src/scorecard/audience.rs` to match.",
            );
        }
    }

    /// Sanity: the signal audit IDs' principle prefixes must exist in the
    /// requirement registry. Guards against a principle-level rename
    /// slipping through even when the audit ID itself stays the same.
    #[test]
    fn signal_ids_reference_known_principles() {
        for signal_id in SIGNAL_AUDIT_IDS {
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
                "signal audit `{signal_id}` has principle prefix `{prefix}` \
                 but no registry requirements match — classifier will \
                 silently drift from spec. Fix `SIGNAL_AUDIT_IDS`.",
            );
        }
    }
}
