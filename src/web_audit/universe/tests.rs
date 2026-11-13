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

/// The variant the row a test names presents, and the group that holds it.
fn design_of(id: &str) -> (&'static AlternativeGroup, &'static AlternativeVariant) {
    let token = antecedent_of(id);
    ALTERNATIVES
        .iter()
        .flat_map(|group| group.variants.iter().map(move |variant| (group, variant)))
        .find(|(_, variant)| variant.presented_by.contains(&token))
        .expect("a design this row presents")
}

fn design_sizes() -> (f64, f64) {
    (
        size_of(design_of("mcp-tools-list").1),
        size_of(design_of("mcp-auth-challenge").1),
    )
}

/// The largest design of a group, which it counts when the site presents none.
fn largest_of(group: &AlternativeGroup) -> f64 {
    group.variants.iter().map(size_of).fold(0.0, f64::max)
}

/// What counts whatever these tests' rows present: every check outside a group of
/// alternatives, and the largest design of each group but theirs.
fn outside_the_group() -> f64 {
    let (theirs, _) = design_of("mcp-tools-list");
    let mut total = weight_of(CHECKS.iter());
    for group in ALTERNATIVES {
        total -= group.variants.iter().map(size_of).sum::<f64>();
        if group.group != theirs.group {
            total += largest_of(group);
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
            base + largest_of(design_of("mcp-tools-list").0),
            base + open_size,
            base + protected_size,
            base + open_size + protected_size
        ),
        "a site counts each design it presents, or the largest when it presents none"
    );
}

#[test]
fn an_n_a_reason_only_an_applied_check_carries_presents_its_design() {
    // The first row's reason presents the protected design beside the open one.
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

// The vendored open design has no checks, so its rows cannot tell a sum from a largest.
// These two designs each hold checks of their own, the smaller presented by the row
// that also carries `AuthRequired`.
const SMALLER: [AntecedentToken; 1] = [AntecedentToken::McpAuth];
const LARGER: [AntecedentToken; 1] = [AntecedentToken::McpResources];
const PRESENTS_SMALLER: [AntecedentToken; 1] = [AntecedentToken::McpSession];
const PRESENTS_LARGER: [AntecedentToken; 1] = [AntecedentToken::McpAuthRequired];
static TWO_DESIGNS: [AlternativeGroup; 1] = [AlternativeGroup {
    group: "two-designs",
    variants: &[
        AlternativeVariant {
            name: "smaller",
            antecedents: &SMALLER,
            presented_by: &PRESENTS_SMALLER,
        },
        AlternativeVariant {
            name: "larger",
            antecedents: &LARGER,
            presented_by: &PRESENTS_LARGER,
        },
    ],
}];

fn two_designs_universe(rows: &[(&'static str, ScorecardStatus, Option<NaReason>)]) -> f64 {
    universe_max(
        CHECKS,
        &TWO_DESIGNS,
        rows.iter().map(|(id, status, na_reason)| UniverseRow {
            id,
            status: *status,
            na_reason: *na_reason,
        }),
        &ScoreConfig::default(),
    )
}

#[test]
fn designs_with_checks_of_their_own_sum_when_presented_and_take_the_largest_when_not() {
    use ScorecardStatus::{NA, Pass};
    let unmet = Some(NaReason::AntecedentUnmet);
    let smaller = size_of(&TWO_DESIGNS[0].variants[0]);
    let larger = size_of(&TWO_DESIGNS[0].variants[1]);
    assert!(0.0 < smaller && smaller < larger, "{smaller} and {larger}");
    let base = weight_of(CHECKS.iter()) - smaller - larger;

    let tools = "mcp-tools-list";
    let challenge = "mcp-auth-challenge";
    assert_eq!(
        two_designs_universe(&[(tools, NA, unmet), (challenge, NA, unmet)]),
        base + larger,
        "none presented counts the largest"
    );
    assert_eq!(
        two_designs_universe(&[(tools, Pass, None), (challenge, NA, unmet)]),
        base + smaller,
        "one presented counts that one"
    );
    assert_eq!(
        two_designs_universe(&[(tools, Pass, None), (challenge, Pass, None)]),
        base + smaller + larger,
        "both presented count both"
    );
    assert_eq!(
        two_designs_universe(&[(tools, NA, Some(NaReason::AuthRequired))]),
        base + larger,
        "a row only blocked by sign-in presents nothing"
    );
}
