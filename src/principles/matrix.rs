//! Coverage matrix generator. Cross-references the requirement registry
//! against the checks discovered at runtime (behavioral + source + project).
//!
//! Output artifacts:
//! - `docs/coverage-matrix.md` — human-readable table grouped by principle.
//! - `coverage/matrix.json` — machine-readable, consumed by the site's
//!   `/coverage` page.
//!
//! The CLI surfaces this as `anc generate coverage-matrix` with `--check`
//! to fail CI when committed artifacts drift from the registry + checks.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;

use crate::check::Check;
use crate::principles::registry::{
    ALL_EXCEPTION_CATEGORIES, Applicability, Level, REQUIREMENTS, SUPPRESSION_TABLE,
};
use crate::types::CheckLayer;

/// A check that covers a given requirement.
#[derive(Debug, Clone, Serialize)]
pub struct Verifier {
    pub check_id: String,
    pub layer: CheckLayer,
}

/// One row of the coverage matrix.
#[derive(Debug, Serialize)]
pub struct MatrixRow {
    pub id: &'static str,
    pub principle: u8,
    pub level: Level,
    pub summary: &'static str,
    pub applicability: Applicability,
    pub verifiers: Vec<Verifier>,
}

/// Programmatic listing of every `--audit-profile` value and what it
/// suppresses. Consumed by the site's regen script and by agents that
/// want to enumerate suppressible checks without scraping `--help`.
#[derive(Debug, Serialize)]
pub struct AuditProfileEntry {
    /// Kebab-case flag value (e.g., `"human-tui"`) — exactly what a
    /// caller passes to `anc check --audit-profile <name>`.
    pub name: &'static str,
    /// One-line human description of the category.
    pub description: &'static str,
    /// Check IDs that emit `Skip` with the audit_profile suppression
    /// prefix when this profile is active. Empty slice = reserved
    /// category with no current suppressions.
    pub suppresses: Vec<&'static str>,
}

/// The rendered matrix, suitable for JSON serialization.
#[derive(Debug, Serialize)]
pub struct Matrix {
    pub schema_version: &'static str,
    pub generated_by: &'static str,
    pub rows: Vec<MatrixRow>,
    pub summary: MatrixSummary,
    /// Every `--audit-profile` category in a stable order. Agents can
    /// read this instead of running `anc check --help` to discover the
    /// valid profile values and what each one excludes.
    pub audit_profiles: Vec<AuditProfileEntry>,
}

#[derive(Debug, Serialize)]
pub struct MatrixSummary {
    pub total: usize,
    pub covered: usize,
    pub uncovered: usize,
    /// Covered requirements that have at least two verifiers, spanning
    /// behavioral + source (or project) layers. Dual-layer coverage is the
    /// headline signal that a requirement is pinned down from more than
    /// one angle — useful for spotting surface-only verifiers.
    pub dual_layer: usize,
    pub must: LevelSummary,
    pub should: LevelSummary,
    pub may: LevelSummary,
}

#[derive(Debug, Serialize)]
pub struct LevelSummary {
    pub total: usize,
    pub covered: usize,
}

const SCHEMA_VERSION: &str = "1.0";
const GENERATED_BY: &str = "anc generate coverage-matrix";

/// Build the matrix from the requirement registry + a slice of checks.
/// Ownership stays with the caller; this reads `check.covers()` references.
pub fn build(checks: &[Box<dyn Check>]) -> Matrix {
    // Inverse map: requirement ID -> Vec<Verifier>.
    let mut coverage: BTreeMap<&'static str, Vec<Verifier>> = BTreeMap::new();
    for check in checks {
        for req_id in check.covers() {
            coverage.entry(req_id).or_default().push(Verifier {
                check_id: check.id().to_string(),
                layer: check.layer(),
            });
        }
    }

    let rows: Vec<MatrixRow> = REQUIREMENTS
        .iter()
        .map(|r| MatrixRow {
            id: r.id,
            principle: r.principle,
            level: r.level,
            summary: r.summary,
            applicability: r.applicability,
            verifiers: coverage.get(r.id).cloned().unwrap_or_default(),
        })
        .collect();

    let summary = summarize(&rows);

    Matrix {
        schema_version: SCHEMA_VERSION,
        generated_by: GENERATED_BY,
        rows,
        summary,
        audit_profiles: build_audit_profiles(),
    }
}

/// Build the `audit_profiles` section of the matrix. Iterates every
/// `ExceptionCategory` variant in the registry order and pairs each with
/// its `SUPPRESSION_TABLE` entry — the order is stable across runs so
/// consumers can diff matrix.json without noise.
fn build_audit_profiles() -> Vec<AuditProfileEntry> {
    ALL_EXCEPTION_CATEGORIES
        .iter()
        .map(|cat| {
            let suppresses: Vec<&'static str> = SUPPRESSION_TABLE
                .iter()
                .find(|(c, _)| *c == *cat)
                .map(|(_, ids)| ids.to_vec())
                .unwrap_or_default();
            AuditProfileEntry {
                name: cat.as_kebab_case(),
                description: cat.description(),
                suppresses,
            }
        })
        .collect()
}

fn summarize(rows: &[MatrixRow]) -> MatrixSummary {
    let mut must = LevelSummary {
        total: 0,
        covered: 0,
    };
    let mut should = LevelSummary {
        total: 0,
        covered: 0,
    };
    let mut may = LevelSummary {
        total: 0,
        covered: 0,
    };
    let mut covered = 0;
    let mut dual_layer = 0;

    for row in rows {
        let bucket = match row.level {
            Level::Must => &mut must,
            Level::Should => &mut should,
            Level::May => &mut may,
        };
        bucket.total += 1;
        if !row.verifiers.is_empty() {
            bucket.covered += 1;
            covered += 1;
            if row.verifiers.len() >= 2 {
                dual_layer += 1;
            }
        }
    }

    MatrixSummary {
        total: rows.len(),
        covered,
        uncovered: rows.len() - covered,
        dual_layer,
        must,
        should,
        may,
    }
}

/// Render the matrix as Markdown. Stable format — a small change in
/// structure will break golden-file tests on purpose.
pub fn render_markdown(matrix: &Matrix) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Coverage Matrix");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "<!-- Generated by `{}` — do not edit by hand. Commit regenerated output alongside code changes. -->",
        GENERATED_BY
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "This table maps every MUST, SHOULD, and MAY in the agent-native CLI spec to the `anc` checks that verify it."
    );
    let _ = writeln!(
        out,
        "When a requirement has no verifier, the cell reads **UNCOVERED** and the reader knows the scorecard cannot speak to it."
    );
    let _ = writeln!(out);

    let s = &matrix.summary;
    let _ = writeln!(out, "## Summary");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "- **Total**: {} requirements ({} covered / {} uncovered)",
        s.total, s.covered, s.uncovered
    );
    let _ = writeln!(
        out,
        "- **Dual-layer**: {} of {} covered requirements have verifiers in two layers (behavioral + source or project)",
        s.dual_layer, s.covered
    );
    let _ = writeln!(
        out,
        "- **MUST**: {} of {} covered",
        s.must.covered, s.must.total
    );
    let _ = writeln!(
        out,
        "- **SHOULD**: {} of {} covered",
        s.should.covered, s.should.total
    );
    let _ = writeln!(
        out,
        "- **MAY**: {} of {} covered",
        s.may.covered, s.may.total
    );
    let _ = writeln!(out);

    // Group rows by principle for readability.
    let mut by_principle: BTreeMap<u8, Vec<&MatrixRow>> = BTreeMap::new();
    for row in &matrix.rows {
        by_principle.entry(row.principle).or_default().push(row);
    }

    for (principle, rows) in &by_principle {
        let _ = writeln!(out, "## P{}: {}", principle, principle_name(*principle));
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "| ID | Level | Applicability | Verifier(s) | Summary |"
        );
        let _ = writeln!(out, "| --- | --- | --- | --- | --- |");
        for row in rows {
            let level = match row.level {
                Level::Must => "MUST",
                Level::Should => "SHOULD",
                Level::May => "MAY",
            };
            let applicability = match row.applicability {
                Applicability::Universal => "Universal".to_string(),
                Applicability::Conditional(cond) => format!("If: {cond}"),
            };
            let verifiers = if row.verifiers.is_empty() {
                "**UNCOVERED**".to_string()
            } else {
                row.verifiers
                    .iter()
                    .map(|v| format!("`{}` ({})", v.check_id, layer_label(v.layer)))
                    .collect::<Vec<_>>()
                    .join("<br>")
            };
            let _ = writeln!(
                out,
                "| `{}` | {} | {} | {} | {} |",
                row.id,
                level,
                applicability,
                verifiers,
                escape_pipes(row.summary)
            );
        }
        let _ = writeln!(out);
    }

    out
}

fn layer_label(layer: CheckLayer) -> &'static str {
    match layer {
        CheckLayer::Behavioral => "behavioral",
        CheckLayer::Source => "source",
        CheckLayer::Project => "project",
    }
}

fn principle_name(principle: u8) -> &'static str {
    match principle {
        1 => "Non-Interactive by Default",
        2 => "Structured, Parseable Output",
        3 => "Progressive Help Discovery",
        4 => "Fail Fast, Actionable Errors",
        5 => "Safe Retries, Mutation Boundaries",
        6 => "Composable, Predictable Command Structure",
        7 => "Bounded, High-Signal Responses",
        _ => "Unknown",
    }
}

/// Replace pipe characters so markdown table rows stay well-formed.
fn escape_pipes(s: &str) -> String {
    s.replace('|', "\\|")
}

/// Render the matrix as pretty JSON.
pub fn render_json(matrix: &Matrix) -> String {
    serde_json::to_string_pretty(matrix).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
}

/// Unreferenced requirement IDs discovered in `Check::covers()`. Used by
/// the registry validator to catch dangling references at test time.
pub fn dangling_cover_ids(checks: &[Box<dyn Check>]) -> Vec<(String, String)> {
    let mut dangling = Vec::new();
    for check in checks {
        for req_id in check.covers() {
            if crate::principles::registry::find(req_id).is_none() {
                dangling.push((check.id().to_string(), (*req_id).to_string()));
            }
        }
    }
    dangling
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::Check;
    use crate::project::Project;
    use crate::types::{CheckGroup, CheckLayer, CheckResult, CheckStatus, Confidence};

    struct FakeCheck {
        id: &'static str,
        covers: &'static [&'static str],
    }

    impl Check for FakeCheck {
        fn id(&self) -> &str {
            self.id
        }
        fn label(&self) -> &'static str {
            "fake"
        }
        fn group(&self) -> CheckGroup {
            CheckGroup::P1
        }
        fn layer(&self) -> CheckLayer {
            CheckLayer::Behavioral
        }
        fn applicable(&self, _project: &Project) -> bool {
            true
        }
        fn run(&self, _project: &Project) -> anyhow::Result<CheckResult> {
            Ok(CheckResult {
                id: self.id.to_string(),
                label: self.id.to_string(),
                group: CheckGroup::P1,
                layer: CheckLayer::Behavioral,
                status: CheckStatus::Pass,
                confidence: Confidence::High,
            })
        }
        fn covers(&self) -> &'static [&'static str] {
            self.covers
        }
    }

    #[test]
    fn build_marks_uncovered_rows_when_no_checks() {
        let checks: Vec<Box<dyn Check>> = vec![];
        let matrix = build(&checks);
        assert_eq!(matrix.rows.len(), REQUIREMENTS.len());
        assert!(matrix.rows.iter().all(|r| r.verifiers.is_empty()));
        assert_eq!(matrix.summary.covered, 0);
        assert_eq!(matrix.summary.uncovered, REQUIREMENTS.len());
    }

    #[test]
    fn build_links_check_to_requirement() {
        let checks: Vec<Box<dyn Check>> = vec![Box::new(FakeCheck {
            id: "fake-check",
            covers: &["p1-must-no-interactive"],
        })];
        let matrix = build(&checks);
        let row = matrix
            .rows
            .iter()
            .find(|r| r.id == "p1-must-no-interactive")
            .expect("requirement row");
        assert_eq!(row.verifiers.len(), 1);
        assert_eq!(row.verifiers[0].check_id, "fake-check");
    }

    #[test]
    fn render_markdown_contains_summary_and_uncovered_marker() {
        let checks: Vec<Box<dyn Check>> = vec![];
        let matrix = build(&checks);
        let md = render_markdown(&matrix);
        assert!(md.contains("# Coverage Matrix"));
        assert!(md.contains("## Summary"));
        assert!(md.contains("**UNCOVERED**"));
        assert!(md.contains("P1: Non-Interactive by Default"));
    }

    #[test]
    fn render_json_is_valid_json() {
        let checks: Vec<Box<dyn Check>> = vec![];
        let matrix = build(&checks);
        let json = render_json(&matrix);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["schema_version"], SCHEMA_VERSION);
        assert!(parsed["rows"].is_array());
    }

    #[test]
    fn dangling_cover_ids_detects_typo() {
        let checks: Vec<Box<dyn Check>> = vec![Box::new(FakeCheck {
            id: "typo-check",
            covers: &["p1-must-no-interactivx"], // typo on purpose
        })];
        let dangling = dangling_cover_ids(&checks);
        assert_eq!(dangling.len(), 1);
        assert_eq!(dangling[0].0, "typo-check");
    }

    #[test]
    fn dangling_cover_ids_empty_for_valid_refs() {
        let checks: Vec<Box<dyn Check>> = vec![Box::new(FakeCheck {
            id: "valid-check",
            covers: &["p1-must-no-interactive", "p1-should-tty-detection"],
        })];
        assert!(dangling_cover_ids(&checks).is_empty());
    }

    /// MUSTs that the spec carries but no behavioral / source / project check
    /// auto-verifies at current scale. Each entry is `(requirement_id, why)`;
    /// the rationale MUST cite the decision record or scope note that
    /// justifies non-coverage.
    ///
    /// Adding to this list is a deliberate act — the test below fails loudly
    /// when a MUST loses its cover (or a new MUST lands in the spec) so a
    /// human has to opt into "we know this isn't auto-checked, here's why."
    const UNVERIFIED_MUSTS: &[(&str, &str)] = &[
        // Pre-existing coverage gaps surfaced by R5 against vendored spec
        // v0.2.0. Each MUST is real and important; the absence of a check
        // reflects scope-of-work, not a stance that the requirement should
        // not be enforced. Track follow-up work in the project roadmap.
        (
            "p2-must-exit-codes",
            "vocabulary check (0, 1, 2, 77, 78 codes appear in mapping) — \
             distinct from `p4-must-exit-code-mapping` which the existing \
             `exit_codes.rs` check covers (mapping shape, not specific values).",
        ),
        (
            "p2-must-json-errors",
            "behavioral check requires inducing an error path with `--output \
             json` and parsing the JSON envelope — no current behavioral \
             check probes error paths.",
        ),
        (
            "p3-must-subcommand-examples",
            "behavioral check would walk `<subcommand> --help` for an \
             `Examples:` section — distinct from `p3-must-top-level-examples` \
             which `p3_examples.rs` covers; per-subcommand traversal is \
             out of scope for v0.1.x.",
        ),
        (
            "p4-must-actionable-errors",
            "judgment-quality check — requires inducing error paths and \
             evaluating message structure (what failed, why, hint). Not \
             reducible to a static-analysis or shape check at current scale.",
        ),
        (
            "p5-must-force-yes",
            "no source check yet detects clap `--force` / `--yes` flag \
             declarations; would mirror `p5-must-dry-run`'s pattern. \
             follow-up work.",
        ),
        (
            "p5-must-read-write-distinction",
            "judgment-quality check — distinguishing read-only from \
             mutating subcommands requires per-subcommand semantic \
             understanding beyond ast-grep's reach. Not auto-verified.",
        ),
    ];

    /// R4 — every `Check::covers()` id in the live catalog resolves in the
    /// generated `REQUIREMENTS` slice. A typo (or a renamed-then-forgotten
    /// id) fails this test rather than silently producing a coverage gap.
    #[test]
    fn live_catalog_has_no_dangling_cover_ids() {
        use crate::checks::all_checks_catalog;

        let checks = all_checks_catalog();
        let dangling = dangling_cover_ids(&checks);
        assert!(
            dangling.is_empty(),
            "checks declare `covers()` ids that are not in REQUIREMENTS: \
             {dangling:?}\nfix `Check::covers()` to reference an id from \
             `src/principles/spec/principles/`."
        );
    }

    /// R5 — every MUST in the vendored spec is covered by at least one
    /// check, OR is explicitly listed in `UNVERIFIED_MUSTS` with rationale.
    #[test]
    fn every_must_is_covered_or_explicitly_unverified() {
        use crate::checks::all_checks_catalog;
        use std::collections::HashSet;

        let checks = all_checks_catalog();
        let covered: HashSet<&'static str> = checks
            .iter()
            .flat_map(|c| c.covers().iter().copied())
            .collect();
        let allowlisted: HashSet<&'static str> =
            UNVERIFIED_MUSTS.iter().map(|(id, _)| *id).collect();

        let gaps: Vec<&'static str> = REQUIREMENTS
            .iter()
            .filter(|r| r.level == Level::Must)
            .map(|r| r.id)
            .filter(|id| !covered.contains(id) && !allowlisted.contains(id))
            .collect();

        assert!(
            gaps.is_empty(),
            "MUSTs without a covering check and not on UNVERIFIED_MUSTS: \
             {gaps:?}\noptions:\n\
             1. wire a check via `Check::covers()` to evidence the MUST, OR\n\
             2. add an entry to UNVERIFIED_MUSTS with a rationale citing the \
                decision record (see docs/decisions/)."
        );
    }

    /// R5 (allowlist hygiene) — every entry in `UNVERIFIED_MUSTS` references
    /// an id that is currently a MUST in the spec and carries a non-empty
    /// rationale. Catches stale shields after a rename or level change.
    #[test]
    fn unverified_musts_allowlist_only_references_real_must_ids() {
        use std::collections::HashSet;

        let must_ids: HashSet<&'static str> = REQUIREMENTS
            .iter()
            .filter(|r| r.level == Level::Must)
            .map(|r| r.id)
            .collect();

        for (id, why) in UNVERIFIED_MUSTS {
            assert!(
                must_ids.contains(id),
                "UNVERIFIED_MUSTS entry `{id}` (`{why}`) is not a current MUST \
                 in REQUIREMENTS — the requirement may have been renamed or \
                 its level changed. update the allowlist or remove the entry."
            );
            assert!(
                !why.trim().is_empty(),
                "UNVERIFIED_MUSTS entry `{id}` has empty rationale"
            );
        }
    }

    #[test]
    fn build_audit_profiles_covers_every_registry_variant() {
        // Emitted list length must equal ALL_EXCEPTION_CATEGORIES length —
        // an ordering or completeness drift on either side fails here.
        let profiles = build_audit_profiles();
        assert_eq!(profiles.len(), ALL_EXCEPTION_CATEGORIES.len());
        for (i, cat) in ALL_EXCEPTION_CATEGORIES.iter().enumerate() {
            assert_eq!(
                profiles[i].name,
                cat.as_kebab_case(),
                "audit_profiles[{i}].name must match registry kebab-case",
            );
            let expected_suppresses: Vec<&'static str> = SUPPRESSION_TABLE
                .iter()
                .find(|(c, _)| *c == *cat)
                .map(|(_, ids)| ids.to_vec())
                .unwrap_or_default();
            assert_eq!(
                profiles[i].suppresses,
                expected_suppresses,
                "audit_profiles[{i}].suppresses must match SUPPRESSION_TABLE for {}",
                cat.as_kebab_case(),
            );
        }
    }

    #[test]
    fn matrix_json_includes_audit_profiles_section() {
        // Build a minimal matrix and verify the rendered JSON has a
        // top-level `audit_profiles` array that downstream consumers can
        // key against without re-running suppression logic.
        let checks: Vec<Box<dyn Check>> = vec![];
        let matrix = build(&checks);
        let json = render_json(&matrix);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let arr = parsed["audit_profiles"]
            .as_array()
            .expect("audit_profiles is a JSON array");
        assert_eq!(arr.len(), ALL_EXCEPTION_CATEGORIES.len());
        for entry in arr {
            assert!(entry["name"].is_string());
            assert!(entry["description"].is_string());
            assert!(entry["suppresses"].is_array());
        }
    }
}
