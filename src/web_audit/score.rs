//! The two-score model, a mirror of the site's `score.ts`.
//!
//! RELATIVE is the headline: earned points over the maximum achievable for
//! this site's applicable checks. GLOBAL is context: earned points over a
//! maximally agent-ready site's maximum, so a bigger correct routine
//! outranks a small perfect one. Per applicable row, at per-tier weights:
//! `pass` earns the weight, `noncompliant` earns a quarter of it, `broken`
//! costs three quarters of it, an absent MUST is a full-weight zero, an
//! absent SHOULD is a zero occupying half its weight in the relative
//! denominator, and `n_a`, `skip` and `error` are excluded from both.
//!
//! Arithmetic follows the site expression for expression, in IEEE doubles,
//! and rounds half up rather than to even, so the vendored parity fixture
//! reproduces on both engines.

use super::registry::WebCheckKeyword;
use super::scorecard::{CategoryRollup, ScorecardStatus};

/// Per-tier point values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScoreWeights {
    /// Weight of a MUST row.
    pub must: f64,
    /// Weight of a SHOULD row.
    pub should: f64,
    /// Weight of a MAY row.
    pub may: f64,
}

impl ScoreWeights {
    /// The weight for a keyword.
    pub fn for_keyword(&self, keyword: WebCheckKeyword) -> f64 {
        match keyword {
            WebCheckKeyword::Must => self.must,
            WebCheckKeyword::Should => self.should,
            WebCheckKeyword::May => self.may,
        }
    }
}

/// The site's default tier weights.
pub const DEFAULT_SCORE_WEIGHTS: ScoreWeights = ScoreWeights {
    must: 5.0,
    should: 3.0,
    may: 1.0,
};
/// Fraction of a row's weight a `broken` surface costs.
pub const DEFAULT_BROKEN_FACTOR: f64 = 0.75;
/// Fraction of a row's weight a `noncompliant` surface earns.
pub const NONCOMPLIANT_CREDIT: f64 = 0.25;

/// The tunable parameters of the model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScoreConfig {
    /// Per-tier weights.
    pub weights: ScoreWeights,
    /// Fraction of the weight a `broken` row costs.
    pub broken_factor: f64,
    /// Fraction of the weight a `noncompliant` row earns.
    pub noncompliant_credit: f64,
}

impl Default for ScoreConfig {
    fn default() -> Self {
        ScoreConfig {
            weights: DEFAULT_SCORE_WEIGHTS,
            broken_factor: DEFAULT_BROKEN_FACTOR,
            noncompliant_credit: NONCOMPLIANT_CREDIT,
        }
    }
}

/// The two scores plus the earned points behind them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WebScore {
    /// Earned over this site's applicable maximum, 0 to 100.
    pub relative: u32,
    /// Earned over the whole registry's maximum, 0 to 100.
    pub global: u32,
    /// Earned points, rounded to one decimal as the site reports it.
    pub earned: f64,
}

/// Whether a status occupies a slot in either score.
pub fn is_scored(status: ScorecardStatus) -> bool {
    matches!(
        status,
        ScorecardStatus::Pass
            | ScorecardStatus::Noncompliant
            | ScorecardStatus::Broken
            | ScorecardStatus::Absent
    )
}

/// Half-up rounding for non-negative operands, the site's `Math.floor(x + 0.5)`.
pub fn round_half_up(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// GLOBAL denominator: every check in the registry at its tier weight.
pub fn universe_max<I>(keywords: I, config: &ScoreConfig) -> f64
where
    I: IntoIterator<Item = WebCheckKeyword>,
{
    keywords.into_iter().fold(0.0, |sum, keyword| {
        sum + config.weights.for_keyword(keyword)
    })
}

fn credit_for(status: ScorecardStatus, config: &ScoreConfig) -> Option<f64> {
    match status {
        ScorecardStatus::Pass => Some(1.0),
        ScorecardStatus::Absent => Some(0.0),
        ScorecardStatus::Broken => Some(-config.broken_factor),
        ScorecardStatus::Noncompliant => Some(config.noncompliant_credit),
        ScorecardStatus::NA | ScorecardStatus::Skip | ScorecardStatus::Error => None,
    }
}

fn pct(earned: f64, max: f64) -> u32 {
    if max > 0.0 {
        let rounded = round_half_up((100.0 * earned) / max).max(0.0);
        // The value is a non-negative integer no larger than 100 by
        // construction, so the conversion cannot truncate.
        rounded as u32
    } else {
        0
    }
}

/// Score a run from each row's keyword and status.
pub fn score_web_audit<I>(rows: I, universe_max: f64, config: &ScoreConfig) -> WebScore
where
    I: IntoIterator<Item = (WebCheckKeyword, ScorecardStatus)>,
{
    let mut earned = 0.0;
    let mut applicable_max = 0.0;
    for (keyword, status) in rows {
        let Some(credit) = credit_for(status, config) else {
            continue;
        };
        let weight = config.weights.for_keyword(keyword);
        earned += weight * credit;
        // An absent SHOULD hurts less than an absent MUST: it occupies only
        // half its weight in the relative denominator. The discount is keyed
        // on absence alone; a noncompliant row carries a real observation.
        applicable_max += if status == ScorecardStatus::Absent && keyword == WebCheckKeyword::Should
        {
            0.5 * weight
        } else {
            weight
        };
    }
    WebScore {
        relative: pct(earned, applicable_max),
        global: pct(earned, universe_max),
        earned: round_half_up(earned * 10.0) / 10.0,
    }
}

/// Per-category `passed/counted` rollups in `category_order`. `counted`
/// counts the scored statuses, so a category of only `n_a` rows is `0/0`.
pub fn category_rollups<'a, I>(
    rows: I,
    category_order: &[&str],
    category_names: &[(&str, &str)],
) -> Vec<CategoryRollup>
where
    I: IntoIterator<Item = (&'a str, ScorecardStatus)>,
{
    let rows: Vec<(&str, ScorecardStatus)> = rows.into_iter().collect();
    category_order
        .iter()
        .map(|id| {
            let mut passed = 0;
            let mut counted = 0;
            for (category, status) in &rows {
                if category != id || !is_scored(*status) {
                    continue;
                }
                counted += 1;
                if *status == ScorecardStatus::Pass {
                    passed += 1;
                }
            }
            let name = category_names
                .iter()
                .find(|(slug, _)| slug == id)
                .map_or(*id, |(_, name)| *name);
            CategoryRollup {
                id: (*id).to_string(),
                name: name.to_string(),
                passed,
                counted,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use WebCheckKeyword::{May, Must, Should};

    fn rows(
        spec: &[(WebCheckKeyword, ScorecardStatus, usize)],
    ) -> Vec<(WebCheckKeyword, ScorecardStatus)> {
        spec.iter()
            .flat_map(|(k, s, n)| std::iter::repeat_n((*k, *s), *n))
            .collect()
    }

    /// A registry-shaped universe: 5 MUST, 15 SHOULD, 16 MAY at default weights.
    fn universe() -> f64 {
        universe_max(
            std::iter::repeat_n(Must, 5)
                .chain(std::iter::repeat_n(Should, 15))
                .chain(std::iter::repeat_n(May, 16)),
            &ScoreConfig::default(),
        )
    }

    #[test]
    fn a_bigger_correct_routine_outranks_a_small_perfect_one_on_global() {
        use ScorecardStatus::{Absent, NA, Pass};
        let cfg = ScoreConfig::default();
        let big = score_web_audit(
            rows(&[
                (Must, Pass, 4),
                (Must, Absent, 1),
                (Should, Pass, 13),
                (Should, Absent, 2),
                (May, Pass, 10),
                (May, NA, 6),
            ]),
            universe(),
            &cfg,
        );
        let small = score_web_audit(
            rows(&[
                (Must, Pass, 2),
                (Should, Pass, 8),
                (May, Pass, 3),
                (May, NA, 13),
            ]),
            universe(),
            &cfg,
        );
        assert_eq!(small.relative, 100);
        assert!(big.relative < 100);
        assert!(big.global > small.global);
    }

    #[test]
    fn broken_costs_more_than_absent_and_noncompliant_sits_between() {
        use ScorecardStatus::{Absent, Broken, Noncompliant, Pass};
        let cfg = ScoreConfig::default();
        for keyword in [Must, Should, May] {
            let at = |status| {
                score_web_audit([(Must, Pass), (keyword, status)], universe(), &cfg).earned
            };
            assert!(at(Pass) > at(Noncompliant), "{keyword:?}");
            assert!(at(Noncompliant) > at(Absent), "{keyword:?}");
            assert!(at(Absent) > at(Broken), "{keyword:?}");
        }
    }

    #[test]
    fn both_scores_floor_at_zero_and_a_lone_broken_must_is_zero() {
        use ScorecardStatus::Broken;
        let cfg = ScoreConfig::default();
        let score = score_web_audit(
            rows(&[(Must, Broken, 3), (Should, Broken, 5)]),
            universe(),
            &cfg,
        );
        assert_eq!((score.relative, score.global), (0, 0));
        // -3.75 reported to one decimal the way JavaScript's Math.round
        // reports it: the half rounds toward positive infinity.
        let lone = score_web_audit([(Must, Broken)], universe(), &cfg);
        assert_eq!(lone.earned, -3.7);
        assert_eq!((lone.relative, lone.global), (0, 0));
    }

    #[test]
    fn unscored_statuses_are_excluded_and_all_n_a_does_not_divide_by_zero() {
        use ScorecardStatus::{Error, NA, Pass, Skip};
        let cfg = ScoreConfig::default();
        let clean = score_web_audit(rows(&[(Must, Pass, 2)]), universe(), &cfg);
        let noisy = score_web_audit(
            rows(&[(Must, Pass, 2), (Should, Skip, 3), (May, Error, 2)]),
            universe(),
            &cfg,
        );
        assert_eq!(clean, noisy);
        let nothing = score_web_audit(rows(&[(Must, NA, 4), (May, NA, 2)]), universe(), &cfg);
        assert_eq!(
            (nothing.relative, nothing.global, nothing.earned),
            (0, 0, 0.0)
        );
        let empty_universe = score_web_audit([(Must, Pass)], 0.0, &cfg);
        assert_eq!(empty_universe.global, 0);
    }

    #[test]
    fn a_noncompliant_row_occupies_its_full_weight_in_the_relative_denominator() {
        use ScorecardStatus::{Absent, Noncompliant};
        let cfg = ScoreConfig::default();
        assert_eq!(
            score_web_audit([(Should, Noncompliant)], universe(), &cfg).relative,
            25
        );
        assert_eq!(
            score_web_audit([(Should, Absent)], universe(), &cfg).relative,
            0
        );
    }

    #[test]
    fn rounding_is_half_up_never_to_even() {
        assert_eq!(round_half_up(0.5), 1.0);
        assert_eq!(round_half_up(1.5), 2.0);
        assert_eq!(round_half_up(2.5), 3.0);
        assert_eq!(round_half_up(2.4999), 2.0);
        // 100 * 1 / 8 = 12.5 rounds up to 13, where round-to-even gives 12.
        let cfg = ScoreConfig {
            weights: ScoreWeights {
                must: 1.0,
                should: 1.0,
                may: 1.0,
            },
            ..ScoreConfig::default()
        };
        let score = score_web_audit(
            rows(&[
                (Must, ScorecardStatus::Pass, 1),
                (Must, ScorecardStatus::Absent, 7),
            ]),
            8.0,
            &cfg,
        );
        assert_eq!((score.relative, score.global), (13, 13));
    }

    #[test]
    fn rollups_follow_category_order_and_count_only_scored_rows() {
        use ScorecardStatus::{Absent, NA, Noncompliant, Pass};
        let rollups = category_rollups(
            [
                ("discoverability", Pass),
                ("discoverability", Absent),
                ("mcp", NA),
                ("mcp", NA),
                ("mcp", Noncompliant),
            ],
            &["discoverability", "mcp", "api"],
            &[("discoverability", "Discoverability"), ("mcp", "MCP")],
        );
        assert_eq!(
            rollups,
            vec![
                CategoryRollup {
                    id: "discoverability".into(),
                    name: "Discoverability".into(),
                    passed: 1,
                    counted: 2
                },
                CategoryRollup {
                    id: "mcp".into(),
                    name: "MCP".into(),
                    passed: 0,
                    counted: 1
                },
                CategoryRollup {
                    id: "api".into(),
                    name: "api".into(),
                    passed: 0,
                    counted: 0
                },
            ]
        );
    }
}
