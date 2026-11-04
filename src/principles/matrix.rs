//! Coverage matrix generator. Cross-references the requirement registry
//! against the audits discovered at runtime (behavioral + source + project).
//!
//! Output artifacts:
//! - `docs/coverage-matrix.md` — human-readable table grouped by principle.
//! - `coverage/matrix.json` — machine-readable, consumed by the site's
//!   `/coverage` page.
//!
//! The CLI surfaces this as `anc emit coverage-matrix` with `--check`
//! to fail CI when committed artifacts drift from the registry + audits.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;

use crate::audit::Audit;
use crate::principles::registry::{
    ALL_EXCEPTION_CATEGORIES, Applicability, Level, REQUIREMENTS, SUPPRESSION_TABLE,
};
use crate::types::AuditLayer;

/// An audit that covers a given requirement.
#[derive(Debug, Clone, Serialize)]
pub struct Verifier {
    pub audit_id: String,
    pub layer: AuditLayer,
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
/// want to enumerate suppressible audits without scraping `--help`.
#[derive(Debug, Serialize)]
pub struct AuditProfileEntry {
    /// Kebab-case flag value (e.g., `"human-tui"`) — exactly what a
    /// caller passes to `anc audit --audit-profile <name>`.
    pub name: &'static str,
    /// One-line human description of the category.
    pub description: &'static str,
    /// Audit IDs that emit `Skip` with the audit_profile suppression
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
    /// read this instead of running `anc audit --help` to discover the
    /// valid profile values and what each one excludes.
    pub audit_profiles: Vec<AuditProfileEntry>,
}

#[derive(Debug, Serialize)]
pub struct MatrixSummary {
    pub total: usize,
    pub covered: usize,
    pub uncovered: usize,
    /// Covered requirements with a behavioral verifier and a source or project
    /// verifier. Dual-layer coverage is the headline signal that a requirement
    /// is pinned down from more than one angle — useful for spotting
    /// surface-only verifiers.
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

const SCHEMA_VERSION: &str = "0.1";
const GENERATED_BY: &str = "anc emit coverage-matrix";

/// Build the matrix from the requirement registry + a slice of audits.
/// Ownership stays with the caller; this reads `audit.covers()` references.
pub fn build(audits: &[Box<dyn Audit>]) -> Matrix {
    // Inverse map: requirement ID -> Vec<Verifier>.
    let mut coverage: BTreeMap<&'static str, Vec<Verifier>> = BTreeMap::new();
    for audit in audits {
        for req_id in audit.covers() {
            coverage.entry(req_id).or_default().push(Verifier {
                audit_id: audit.id().to_string(),
                layer: audit.layer(),
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
            let has_layer = |layer| row.verifiers.iter().any(|v| v.layer == layer);
            if has_layer(AuditLayer::Behavioral)
                && (has_layer(AuditLayer::Source) || has_layer(AuditLayer::Project))
            {
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
        "<!-- Generated by `{}`; do not edit by hand. Commit regenerated output alongside code changes. -->",
        GENERATED_BY
    );
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "This table maps every MUST, SHOULD, and MAY in the agent-native CLI spec to the `anc` audits that verify it."
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
                Applicability::Conditional {
                    condition,
                    antecedent,
                } => match (condition, antecedent) {
                    (Some(cond), Some(ante)) => {
                        format!("If: {cond} (antecedent: `{}`)", ante.audit_id)
                    }
                    (Some(cond), None) => format!("If: {cond}"),
                    (None, Some(ante)) => format!("If: `{}` is present", ante.audit_id),
                    (None, None) => "Conditional".to_string(),
                },
            };
            let verifiers = if row.verifiers.is_empty() {
                "**UNCOVERED**".to_string()
            } else {
                row.verifiers
                    .iter()
                    .map(|v| format!("`{}` ({})", v.audit_id, layer_label(v.layer)))
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

    // The blank line closing each principle is a separator, so the last one
    // lands at EOF. markdownlint's MD047 wants a single trailing newline and
    // the pre-commit pass runs with `fix: true`, which would rewrite this file
    // in the working tree right after the release commit stages it.
    while out.ends_with("\n\n") {
        out.pop();
    }

    out
}

fn layer_label(layer: AuditLayer) -> &'static str {
    match layer {
        AuditLayer::Behavioral => "behavioral",
        AuditLayer::Source => "source",
        AuditLayer::Project => "project",
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
        8 => "Discoverable Through Agent Skill Bundles",
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

/// Unreferenced requirement IDs discovered in `Audit::covers()`. Used by
/// the registry validator to catch dangling references at test time.
pub fn dangling_cover_ids(audits: &[Box<dyn Audit>]) -> Vec<(String, String)> {
    let mut dangling = Vec::new();
    for audit in audits {
        for req_id in audit.covers() {
            if crate::principles::registry::find(req_id).is_none() {
                dangling.push((audit.id().to_string(), (*req_id).to_string()));
            }
        }
    }
    dangling
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::Audit;
    use crate::project::Project;
    use crate::types::{AuditGroup, AuditLayer, AuditResult, AuditStatus, Confidence};

    struct FakeAudit {
        id: &'static str,
        covers: &'static [&'static str],
    }

    impl Audit for FakeAudit {
        fn id(&self) -> &str {
            self.id
        }
        fn label(&self) -> &'static str {
            "fake"
        }
        fn group(&self) -> AuditGroup {
            AuditGroup::P1
        }
        fn layer(&self) -> AuditLayer {
            AuditLayer::Behavioral
        }
        fn applicable(&self, _project: &Project) -> bool {
            true
        }
        fn run(&self, _project: &Project) -> anyhow::Result<AuditResult> {
            Ok(AuditResult {
                id: self.id.to_string(),
                label: self.id.to_string(),
                group: AuditGroup::P1,
                layer: AuditLayer::Behavioral,
                status: AuditStatus::Pass,
                confidence: Confidence::High,
                mitigation: None,
                config_hint: None,
                row_statuses: Vec::new(),
                pass_evidence: None,
            })
        }
        fn covers(&self) -> &'static [&'static str] {
            self.covers
        }
    }

    #[test]
    fn build_marks_uncovered_rows_when_no_audits() {
        let audits: Vec<Box<dyn Audit>> = vec![];
        let matrix = build(&audits);
        assert_eq!(matrix.rows.len(), REQUIREMENTS.len());
        assert!(matrix.rows.iter().all(|r| r.verifiers.is_empty()));
        assert_eq!(matrix.summary.covered, 0);
        assert_eq!(matrix.summary.uncovered, REQUIREMENTS.len());
    }

    #[test]
    fn build_links_audit_to_requirement() {
        let audits: Vec<Box<dyn Audit>> = vec![Box::new(FakeAudit {
            id: "fake-audit",
            covers: &["p1-must-no-interactive"],
        })];
        let matrix = build(&audits);
        let row = matrix
            .rows
            .iter()
            .find(|r| r.id == "p1-must-no-interactive")
            .expect("requirement row");
        assert_eq!(row.verifiers.len(), 1);
        assert_eq!(row.verifiers[0].audit_id, "fake-audit");
    }

    #[test]
    fn render_markdown_contains_summary_and_uncovered_marker() {
        let audits: Vec<Box<dyn Audit>> = vec![];
        let matrix = build(&audits);
        let md = render_markdown(&matrix);
        assert!(md.contains("# Coverage Matrix"));
        assert!(md.contains("## Summary"));
        assert!(md.contains("**UNCOVERED**"));
        assert!(md.contains("P1: Non-Interactive by Default"));
    }

    // A principle the registry grades but `principle_name` does not know would
    // publish a `## P9: Unknown` heading in the committed matrix.
    #[test]
    fn every_registry_principle_has_a_name() {
        for requirement in REQUIREMENTS {
            assert_ne!(
                principle_name(requirement.principle),
                "Unknown",
                "`{}` belongs to a principle `principle_name` does not know",
                requirement.id
            );
        }
    }

    #[test]
    fn p8_heading_carries_the_vendored_principle_title() {
        let title = include_str!("spec/principles/p8-discoverable-skill-bundle.md")
            .lines()
            .find(|line| line.starts_with("# P8: "))
            .expect("the vendored P8 file has a `# P8:` title");
        let md = render_markdown(&build(&[]));
        assert!(
            md.lines().any(|line| line == format!("#{title}")),
            "no `#{title}` heading in the rendered matrix"
        );
    }

    #[test]
    fn bundle_requirement_has_one_verifier() {
        let matrix = build(&crate::audits::all_audits_catalog());
        let row = matrix
            .rows
            .iter()
            .find(|r| r.id == "p8-should-bundle-exists")
            .expect("requirement row");
        let verifiers: Vec<&str> = row.verifiers.iter().map(|v| v.audit_id.as_str()).collect();
        assert_eq!(verifiers, ["p8-bundle-exists"]);
    }

    // A Rust audit and a Python audit that share an id are two source verifiers
    // of one requirement, which is one layer however many verifiers it has.
    #[test]
    fn dual_layer_counts_behavioral_plus_source_or_project() {
        use AuditLayer::{Behavioral, Project, Source};

        let cases: [(&[AuditLayer], usize); 8] = [
            (&[Behavioral, Source], 1),
            (&[Behavioral, Project], 1),
            (&[Behavioral, Behavioral, Project], 1),
            (&[Behavioral, Source, Source], 1),
            (&[Source, Source], 0),
            (&[Project, Source], 0),
            (&[Behavioral, Behavioral], 0),
            (&[Behavioral], 0),
        ];
        for (layers, expected) in cases {
            let mut matrix = build(&[]);
            matrix.rows[0].verifiers = layers
                .iter()
                .map(|&layer| Verifier {
                    audit_id: "fake-audit".to_string(),
                    layer,
                })
                .collect();
            assert_eq!(
                summarize(&matrix.rows).dual_layer,
                expected,
                "verifier layers {layers:?}"
            );
        }
    }

    // `--principle <N>` filters on an audit's group, so an audit grouped under
    // one principle that covers another's requirement reports that requirement
    // under the wrong principle.
    #[test]
    fn audits_grouped_under_a_principle_cover_only_that_principle() {
        let mut misplaced = Vec::new();
        for audit in crate::audits::all_audits_catalog() {
            let group = format!("{:?}", audit.group());
            let Some(principle) = group.strip_prefix('P').and_then(|n| n.parse::<u8>().ok()) else {
                continue;
            };
            for id in audit.covers() {
                let requirement = crate::principles::registry::find(id).expect("registry id");
                if requirement.principle != principle {
                    misplaced.push(format!("{} ({group}) covers {id}", audit.id()));
                }
            }
        }
        assert!(misplaced.is_empty(), "{misplaced:#?}");
    }

    // The committed `docs/coverage-matrix.md` is staged during a release cut,
    // and the pre-commit markdownlint pass runs with `fix: true`, so a trailing
    // blank line here is rewritten out of the working tree right after the
    // commit that added it. Emitting one newline leaves the hook nothing to fix.
    #[test]
    fn render_markdown_ends_with_exactly_one_newline() {
        let audits: Vec<Box<dyn Audit>> = vec![];
        let matrix = build(&audits);
        let md = render_markdown(&matrix);
        assert!(
            md.ends_with('\n'),
            "rendered markdown must end with a newline"
        );
        assert!(
            !md.ends_with("\n\n"),
            "rendered markdown must not end with a blank line; got {:?} at the tail",
            &md[md.len().saturating_sub(40)..]
        );
    }

    #[test]
    fn render_json_is_valid_json() {
        let audits: Vec<Box<dyn Audit>> = vec![];
        let matrix = build(&audits);
        let json = render_json(&matrix);
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(parsed["schema_version"], SCHEMA_VERSION);
        assert!(parsed["rows"].is_array());
    }

    #[test]
    fn dangling_cover_ids_detects_typo() {
        let audits: Vec<Box<dyn Audit>> = vec![Box::new(FakeAudit {
            id: "typo-audit",
            covers: &["p1-must-no-interactivx"], // typo on purpose
        })];
        let dangling = dangling_cover_ids(&audits);
        assert_eq!(dangling.len(), 1);
        assert_eq!(dangling[0].0, "typo-audit");
    }

    #[test]
    fn dangling_cover_ids_empty_for_valid_refs() {
        let audits: Vec<Box<dyn Audit>> = vec![Box::new(FakeAudit {
            id: "valid-audit",
            covers: &["p1-must-no-interactive", "p1-should-tty-detection"],
        })];
        assert!(dangling_cover_ids(&audits).is_empty());
    }

    /// MUSTs that the spec carries but no behavioral / source / project audit
    /// auto-verifies at current scale. Each entry is `(requirement_id, why)`;
    /// the rationale MUST cite the decision record or scope note that
    /// justifies non-coverage.
    ///
    /// Adding to this list is a deliberate act — the test below fails loudly
    /// when a MUST loses its cover (or a new MUST lands in the spec) so a
    /// human has to opt into "we know this isn't auto-audited, here's why."
    const UNVERIFIED_MUSTS: &[(&str, &str)] = &[];

    /// R4 — every `Audit::covers()` id in the live catalog resolves in the
    /// generated `REQUIREMENTS` slice. A typo (or a renamed-then-forgotten
    /// id) fails this test rather than silently producing a coverage gap.
    #[test]
    fn live_catalog_has_no_dangling_cover_ids() {
        use crate::audits::all_audits_catalog;

        let audits = all_audits_catalog();
        let dangling = dangling_cover_ids(&audits);
        assert!(
            dangling.is_empty(),
            "audits declare `covers()` ids that are not in REQUIREMENTS: \
             {dangling:?}\nfix `Audit::covers()` to reference an id from \
             `src/principles/spec/principles/`."
        );
    }

    /// R5 — every MUST in the vendored spec is covered by at least one
    /// audit, OR is explicitly listed in `UNVERIFIED_MUSTS` with rationale.
    #[test]
    fn every_must_is_covered_or_explicitly_unverified() {
        use crate::audits::all_audits_catalog;
        use std::collections::HashSet;

        let audits = all_audits_catalog();
        let covered: HashSet<&'static str> = audits
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
            "MUSTs without a covering audit and not on UNVERIFIED_MUSTS: \
             {gaps:?}\noptions:\n\
             1. wire an audit via `Audit::covers()` to evidence the MUST, OR\n\
             2. add an entry to UNVERIFIED_MUSTS with a rationale citing the \
                decision record (see docs/decisions/)."
        );
    }

    /// R5 (allowlist hygiene) — every entry in `UNVERIFIED_MUSTS` references
    /// an id that is currently a MUST in the spec, carries a non-empty
    /// rationale, and has no audit covering it. Catches stale shields after a
    /// rename, a level change, or an audit landing for the MUST.
    #[test]
    fn unverified_musts_allowlist_only_references_real_must_ids() {
        use crate::audits::all_audits_catalog;
        use std::collections::HashSet;

        let must_ids: HashSet<&'static str> = REQUIREMENTS
            .iter()
            .filter(|r| r.level == Level::Must)
            .map(|r| r.id)
            .collect();
        let covered: HashSet<&'static str> = all_audits_catalog()
            .iter()
            .flat_map(|c| c.covers().iter().copied())
            .collect();

        for (id, why) in UNVERIFIED_MUSTS {
            assert!(
                !covered.contains(id),
                "UNVERIFIED_MUSTS entry `{id}` is covered by an audit's \
                 `covers()`, so it is verified: remove the entry."
            );
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
        let audits: Vec<Box<dyn Audit>> = vec![];
        let matrix = build(&audits);
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
