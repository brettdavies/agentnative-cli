//! The global score's denominator, a mirror of `universeMaxOf` in the
//! site's `score.ts`: the most a single site could earn. Alternatives are
//! site designs, never access limits, so a check sign-in blocks stays in
//! the maximum while a design the site does not present leaves it.

use std::collections::HashSet;

use super::registry::{AlternativeGroup, AntecedentToken, WebCheck};
use super::score::ScoreConfig;
use super::scorecard::{NaReason, ScorecardStatus};

/// The row fields the global universe reads; a stored row carries each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UniverseRow<'a> {
    /// The check id.
    pub id: &'a str,
    /// The row's status.
    pub status: ScorecardStatus,
    /// Why the row is `n_a`.
    pub na_reason: Option<NaReason>,
}

fn row_applied(row: &UniverseRow<'_>) -> bool {
    row.status != ScorecardStatus::NA || row.na_reason.is_some_and(NaReason::only_when_applicable)
}

/// The variant a token names, the last declaration winning as the site's
/// `Map.set` does.
fn variant_of(
    index: &[(AntecedentToken, (usize, usize))],
    token: AntecedentToken,
) -> Option<(usize, usize)> {
    index
        .iter()
        .rev()
        .find(|(t, _)| *t == token)
        .map(|(_, at)| *at)
}

/// Every check counts at its tier weight whatever its row reads, except in
/// a group of alternatives, which counts each variant the site presents (a
/// row gated on one of the variant's presence tokens shows its check
/// applied), or its largest variant when the site presents none.
/// Presentation is read from the rows alone, so a stored scorecard
/// recomputes the denominator it was scored under.
pub fn universe_max<'a>(
    checks: &[WebCheck],
    alternatives: &[AlternativeGroup],
    rows: impl IntoIterator<Item = UniverseRow<'a>>,
    config: &ScoreConfig,
) -> f64 {
    let applied: HashSet<&str> = rows
        .into_iter()
        .filter(row_applied)
        .map(|row| row.id)
        .collect();
    let mut size: Vec<Vec<f64>> = alternatives
        .iter()
        .map(|g| vec![0.0; g.variants.len()])
        .collect();
    let mut presented: Vec<Vec<bool>> = alternatives
        .iter()
        .map(|g| vec![false; g.variants.len()])
        .collect();
    let mut member_of = Vec::new();
    let mut presented_via = Vec::new();
    for (g, group) in alternatives.iter().enumerate() {
        for (v, variant) in group.variants.iter().enumerate() {
            member_of.extend(variant.antecedents.iter().map(|t| (*t, (g, v))));
            presented_via.extend(variant.presented_by.iter().map(|t| (*t, (g, v))));
        }
    }
    let mut total = 0.0;
    for check in checks {
        let weight = config.weights.for_keyword(check.keyword);
        match variant_of(&member_of, check.antecedent) {
            None => total += weight,
            Some((g, v)) => size[g][v] += weight,
        }
        if let Some((g, v)) = variant_of(&presented_via, check.antecedent)
            && applied.contains(check.id)
        {
            presented[g][v] = true;
        }
    }
    for (sizes, shown) in size.iter().zip(&presented) {
        total += if shown.iter().any(|s| *s) {
            sizes
                .iter()
                .zip(shown)
                .filter(|(_, s)| **s)
                .map(|(size, _)| size)
                .sum::<f64>()
        } else {
            sizes.iter().copied().fold(0.0, f64::max)
        };
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web_audit::registry::{ALTERNATIVES, AlternativeVariant, CHECKS};

    fn universe(rows: &[(&'static str, ScorecardStatus, Option<NaReason>)]) -> f64 {
        universe_max(
            CHECKS,
            ALTERNATIVES,
            rows.iter().map(|(id, status, na_reason)| UniverseRow {
                id,
                status: *status,
                na_reason: *na_reason,
            }),
            &ScoreConfig::default(),
        )
    }

    fn weight_of(checks: impl Iterator<Item = &'static WebCheck>) -> f64 {
        checks
            .map(|c| ScoreConfig::default().weights.for_keyword(c.keyword))
            .sum()
    }

    fn antecedent_of(id: &str) -> AntecedentToken {
        CHECKS
            .iter()
            .find(|c| c.id == id)
            .expect("a check this test names")
            .antecedent
    }

    fn size_of(variant: &AlternativeVariant) -> f64 {
        weight_of(
            CHECKS
                .iter()
                .filter(|c| variant.antecedents.contains(&c.antecedent)),
        )
    }

    /// The variant the row a test names presents.
    fn design_of(id: &str) -> &'static AlternativeVariant {
        let token = antecedent_of(id);
        ALTERNATIVES
            .iter()
            .flat_map(|group| group.variants)
            .find(|variant| variant.presented_by.contains(&token))
            .expect("a design this row presents")
    }

    fn design_sizes() -> (f64, f64) {
        (
            size_of(design_of("mcp-tools-list")),
            size_of(design_of("mcp-auth-challenge")),
        )
    }

    /// What counts whatever these tests' rows present: every check outside a group of
    /// alternatives, and the largest design of each group but theirs.
    fn outside_the_group() -> f64 {
        let open = design_of("mcp-tools-list");
        let mut total = weight_of(CHECKS.iter());
        for group in ALTERNATIVES {
            let sizes: Vec<f64> = group.variants.iter().map(size_of).collect();
            total -= sizes.iter().sum::<f64>();
            if !group.variants.iter().any(|variant| variant == open) {
                total += sizes.iter().copied().fold(0.0, f64::max);
            }
        }
        total
    }

    #[test]
    fn the_mcp_access_group_counts_the_design_the_site_presents() {
        use ScorecardStatus::{NA, Pass};
        let unmet = Some(NaReason::AntecedentUnmet);
        let no_mcp = universe(&[
            ("mcp-tools-list", NA, unmet),
            ("mcp-auth-challenge", NA, unmet),
        ]);
        let open = universe(&[
            ("mcp-tools-list", Pass, None),
            ("mcp-auth-challenge", NA, unmet),
        ]);
        let protected = universe(&[
            ("mcp-tools-list", NA, Some(NaReason::AuthRequired)),
            ("mcp-auth-challenge", Pass, None),
        ]);
        let hybrid = universe(&[
            ("mcp-tools-list", Pass, None),
            ("mcp-auth-challenge", Pass, None),
        ]);
        let (open_size, protected_size) = design_sizes();
        let base = outside_the_group();
        assert_eq!(
            (no_mcp, open, protected, hybrid),
            (
                base + open_size.max(protected_size),
                base + open_size,
                base + protected_size,
                base + open_size + protected_size
            ),
            "a site counts each design it presents, or the largest when it presents none"
        );
    }

    #[test]
    fn an_n_a_reason_only_an_applied_check_carries_presents_its_design() {
        // Without the row's reason only the open design is presented, so this reads
        // the smaller maximum.
        let optional_absent = universe(&[
            ("mcp-tools-list", ScorecardStatus::Pass, None),
            (
                "mcp-auth-challenge",
                ScorecardStatus::NA,
                Some(NaReason::OptionalAbsent),
            ),
        ]);
        let auth_required = universe(&[(
            "mcp-tools-list",
            ScorecardStatus::NA,
            Some(NaReason::AuthRequired),
        )]);
        let no_mcp = universe(&[
            (
                "mcp-tools-list",
                ScorecardStatus::NA,
                Some(NaReason::AntecedentUnmet),
            ),
            (
                "mcp-auth-challenge",
                ScorecardStatus::NA,
                Some(NaReason::AntecedentUnmet),
            ),
        ]);
        let (open_size, protected_size) = design_sizes();
        assert_eq!(
            (optional_absent, auth_required),
            (outside_the_group() + open_size + protected_size, no_mcp)
        );
    }

    #[test]
    fn without_alternatives_every_check_counts() {
        let all: f64 = CHECKS
            .iter()
            .map(|c| ScoreConfig::default().weights.for_keyword(c.keyword))
            .sum();
        let none = universe_max(CHECKS, &[], [], &ScoreConfig::default());
        assert_eq!(none, all);
    }
}
