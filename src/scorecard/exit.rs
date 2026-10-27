//! The exit code an `anc audit` run earns, and the reason its text report
//! closes on. The table itself is [`crate::exit_codes`], shared by every
//! verb.

use crate::exit_codes::{EXIT_CLEAN, EXIT_COULD_NOT_CHECK, EXIT_FAILURES, EXIT_WARNINGS, plural};
use crate::types::{AuditResult, AuditStatus};

/// An audit that reached a verdict on the tool. `Skip` (a probe limit or an
/// `--audit-profile` suppression) and `NotApplicable` reached none.
fn applied(status: &AuditStatus) -> bool {
    matches!(
        status,
        AuditStatus::Pass | AuditStatus::Warn(_) | AuditStatus::Fail(_) | AuditStatus::OptOut(_)
    )
}

fn errored(status: &AuditStatus) -> bool {
    matches!(status, AuditStatus::Error(_))
}

/// Derive the process exit code from the full result set.
///
/// - `3`: an audit errored, or none applied. An error outranks a failure:
///   a run that could not check part of the tool does not report the rest
///   as its verdict.
/// - `2`: at least one Fail.
/// - `1`: at least one Warn.
/// - `0`: otherwise.
///
/// **`--audit-profile` affects the exit code by masking Fails to Skips.**
/// An audit that would otherwise Fail but is suppressed by the applied
/// profile contributes nothing and cannot lift the code above `0`/`1`. This
/// is intentional per plan R4: the caller is declaring "this category of
/// audit doesn't apply to this tool", so scoring against that requirement
/// would produce a misleading non-zero exit. The tradeoff is that callers
/// passing the wrong profile can silently bless a broken tool; guarding
/// against that lives upstream (site's regen script, CI policy), not here.
pub fn exit_code(results: &[AuditResult]) -> i32 {
    if results.iter().any(|r| errored(&r.status)) || !results.iter().any(|r| applied(&r.status)) {
        EXIT_COULD_NOT_CHECK
    } else if results
        .iter()
        .any(|r| matches!(r.status, AuditStatus::Fail(_)))
    {
        EXIT_FAILURES
    } else if results
        .iter()
        .any(|r| matches!(r.status, AuditStatus::Warn(_)))
    {
        EXIT_WARNINGS
    } else {
        EXIT_CLEAN
    }
}

/// What earned an exit code, for the report's closing line.
pub fn exit_reason(code: i32, results: &[AuditResult]) -> String {
    let count = |pred: fn(&AuditStatus) -> bool| results.iter().filter(|r| pred(&r.status)).count();
    match code {
        EXIT_CLEAN => "every applicable audit passed".to_string(),
        EXIT_WARNINGS => {
            let n = count(|s| matches!(s, AuditStatus::Warn(_)));
            format!("warnings only ({n} {})", plural(n, "warning", "warnings"))
        }
        EXIT_FAILURES => {
            let n = count(|s| matches!(s, AuditStatus::Fail(_)));
            format!(
                "failures present ({n} {})",
                plural(n, "failure", "failures")
            )
        }
        _ => {
            let n = count(errored);
            if n == 0 {
                "could not check: no audit applied".to_string()
            } else {
                format!(
                    "could not check: {n} {} errored",
                    plural(n, "audit", "audits")
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AuditGroup, AuditLayer, Confidence};

    fn result(status: AuditStatus) -> AuditResult {
        AuditResult {
            id: "a".into(),
            label: "A".into(),
            group: AuditGroup::P1,
            layer: AuditLayer::Behavioral,
            status,
            confidence: Confidence::High,
            mitigation: None,
            config_hint: None,
            pass_evidence: None,
        }
    }

    #[test]
    fn the_four_codes_follow_the_shared_table() {
        use AuditStatus as S;
        let code =
            |statuses: Vec<S>| exit_code(&statuses.into_iter().map(result).collect::<Vec<_>>());
        assert_eq!(code(vec![S::Pass, S::Skip("n/a".into())]), 0);
        assert_eq!(code(vec![S::Pass, S::OptOut("declined".into())]), 0);
        assert_eq!(code(vec![S::Pass, S::Warn("meh".into())]), 1);
        assert_eq!(code(vec![S::Fail("bad".into()), S::Warn("meh".into())]), 2);
        assert_eq!(code(vec![S::Error("boom".into())]), 3);
        assert_eq!(
            code(vec![S::Fail("bad".into()), S::Error("boom".into())]),
            3
        );
        assert_eq!(
            code(vec![
                S::Skip("probe".into()),
                S::NotApplicable("gate".into())
            ]),
            3
        );
        assert_eq!(code(vec![]), 3);
    }

    #[test]
    fn the_closing_reason_names_what_earned_the_code() {
        use AuditStatus as S;
        let results: Vec<AuditResult> = vec![
            result(S::Fail("bad".into())),
            result(S::Error("boom".into())),
            result(S::Error("bust".into())),
        ];
        assert_eq!(
            exit_reason(3, &results),
            "could not check: 2 audits errored"
        );
        assert_eq!(
            exit_reason(3, &[result(S::Skip("probe".into()))]),
            "could not check: no audit applied"
        );
        assert_eq!(
            exit_reason(2, &[result(S::Fail("bad".into()))]),
            "failures present (1 failure)"
        );
        assert_eq!(
            exit_reason(
                1,
                &[result(S::Warn("a".into())), result(S::Warn("b".into()))]
            ),
            "warnings only (2 warnings)"
        );
        assert_eq!(
            exit_reason(0, &[result(S::Pass)]),
            "every applicable audit passed"
        );
    }
}
