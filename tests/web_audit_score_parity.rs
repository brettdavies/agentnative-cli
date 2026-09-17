//! Parity of the scorecard mirror and the two-score model with the anc.dev
//! engine: the vendored scoring fixture reproduces its expected scores, a
//! perturbed weight breaks it, every corpus golden round-trips byte for
//! byte through the strict mirror, and the committed JSON Schema describes
//! exactly what the mirror serializes.
//!
//! The modules are pulled in by path under a `web_audit` shim so their
//! `crate::web_audit::*` references resolve without a library target.

#[allow(dead_code)]
#[path = "../src/web_audit"]
mod web_audit {
    #[path = "registry.rs"]
    pub mod registry;
    #[path = "score.rs"]
    pub mod score;
    #[path = "scorecard.rs"]
    pub mod scorecard;
}

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use web_audit::registry::{CHECKS, WebCheckKeyword};
use web_audit::score::{self, ScoreConfig, ScoreWeights};
use web_audit::scorecard::{NaReason, ScorecardStatus, WebScorecard, to_wire_json};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn keyword(s: &str) -> WebCheckKeyword {
    match s {
        "must" => WebCheckKeyword::Must,
        "should" => WebCheckKeyword::Should,
        "may" => WebCheckKeyword::May,
        other => panic!("fixture keyword {other}"),
    }
}

fn status(s: &str) -> ScorecardStatus {
    serde_json::from_value(Value::String(s.to_string())).unwrap_or_else(|e| panic!("{s}: {e}"))
}

struct Fixture {
    config: ScoreConfig,
    universe: Vec<WebCheckKeyword>,
    rows: Vec<(WebCheckKeyword, ScorecardStatus)>,
    expected: (u32, u32),
}

fn parity_fixture() -> Fixture {
    let raw: Value = serde_json::from_str(
        &fs::read_to_string(fixtures().join("web-audit-score-parity.json")).unwrap(),
    )
    .unwrap();
    let w = &raw["weights"];
    Fixture {
        config: ScoreConfig {
            weights: ScoreWeights {
                must: w["must"].as_f64().unwrap(),
                should: w["should"].as_f64().unwrap(),
                may: w["may"].as_f64().unwrap(),
            },
            broken_factor: raw["broken_factor"].as_f64().unwrap(),
            noncompliant_credit: raw["noncompliant_credit"].as_f64().unwrap(),
        },
        universe: raw["universe_tiers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| keyword(v.as_str().unwrap()))
            .collect(),
        rows: raw["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| {
                (
                    keyword(pair[0].as_str().unwrap()),
                    status(pair[1].as_str().unwrap()),
                )
            })
            .collect(),
        expected: (
            raw["expected"]["relative"].as_u64().unwrap() as u32,
            raw["expected"]["global"].as_u64().unwrap() as u32,
        ),
    }
}

#[test]
fn the_vendored_parity_fixture_reproduces_its_expected_scores() {
    let f = parity_fixture();
    let universe = score::universe_max(f.universe.iter().copied(), &f.config);
    let scored = score::score_web_audit(f.rows.iter().copied(), universe, &f.config);
    assert_eq!((scored.relative, scored.global), f.expected);
}

#[test]
fn a_perturbed_weight_fails_the_fixture() {
    let f = parity_fixture();
    let perturbed = ScoreConfig {
        weights: ScoreWeights {
            must: f.config.weights.must + 1.0,
            ..f.config.weights
        },
        ..f.config
    };
    let universe = score::universe_max(f.universe.iter().copied(), &perturbed);
    let scored = score::score_web_audit(f.rows.iter().copied(), universe, &perturbed);
    assert_ne!((scored.relative, scored.global), f.expected);
}

#[test]
fn an_unknown_status_string_fails_to_deserialize_naming_the_value() {
    let err = serde_json::from_value::<ScorecardStatus>(Value::String("warn".into())).unwrap_err();
    assert!(err.to_string().contains("warn"), "{err}");
    let err = serde_json::from_value::<NaReason>(Value::String("because".into())).unwrap_err();
    assert!(err.to_string().contains("because"), "{err}");
}

#[test]
fn an_unknown_field_fails_to_deserialize_naming_the_field() {
    let golden = first_golden();
    let mut doc: Value = serde_json::from_str(&golden).unwrap();
    doc["results"][0]["remediation"] = Value::String("x".into());
    let err = serde_json::from_value::<WebScorecard>(doc).unwrap_err();
    assert!(err.to_string().contains("remediation"), "{err}");
    let mut doc: Value = serde_json::from_str(&golden).unwrap();
    doc["score"]["relative"] = Value::from(12.5);
    let err = serde_json::from_value::<WebScorecard>(doc).unwrap_err();
    assert!(err.to_string().contains("12.5"), "{err}");
}

fn golden_paths() -> Vec<PathBuf> {
    let dir = fixtures().join("web-audit-conformance/scenarios");
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|entry| entry.unwrap().path().join("scorecard.json"))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    assert!(
        paths.len() > 50,
        "the corpus has {} scorecards",
        paths.len()
    );
    paths
}

fn first_golden() -> String {
    golden_paths()
        .into_iter()
        .map(|p| fs::read_to_string(p).unwrap())
        .find(|text| !text.starts_with("{\n  \"unreachable\""))
        .expect("a scorecard golden")
}

#[test]
fn every_corpus_golden_round_trips_byte_for_byte() {
    let mut compared = 0;
    for path in golden_paths() {
        let text = fs::read_to_string(&path).unwrap();
        if text.starts_with("{\n  \"unreachable\"") {
            continue;
        }
        let scorecard: WebScorecard =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let ours = to_wire_json(&scorecard);
        assert!(
            ours == text,
            "{} differs:\n{}",
            path.display(),
            first_difference(&text, &ours)
        );
        let parsed: Value = serde_json::from_str(&ours).unwrap();
        assert_integers_only(&parsed, &path);
        compared += 1;
    }
    assert!(compared > 50, "compared {compared} goldens");
}

/// Every number in a scorecard is an integer; a float anywhere is a mirror
/// that has drifted from the site's integer-only contract.
fn assert_integers_only(value: &Value, path: &Path) {
    match value {
        Value::Number(n) => assert!(
            n.is_u64() || n.is_i64(),
            "{}: non-integer {n}",
            path.display()
        ),
        Value::Array(items) => items.iter().for_each(|v| assert_integers_only(v, path)),
        Value::Object(map) => map.values().for_each(|v| assert_integers_only(v, path)),
        _ => {}
    }
}

fn first_difference(expected: &str, actual: &str) -> String {
    for (index, (e, a)) in expected.lines().zip(actual.lines()).enumerate() {
        if e != a {
            let line = index + 1;
            return format!("line {line}\n  expected: {e}\n  actual:   {a}");
        }
    }
    format!(
        "lengths differ: expected {} lines, actual {}",
        expected.lines().count(),
        actual.lines().count()
    )
}

/// The keys a struct serializes, from the first golden.
fn keys(v: &Value) -> BTreeSet<String> {
    v.as_object().unwrap().keys().cloned().collect()
}

#[test]
fn the_committed_schema_describes_exactly_what_the_mirror_serializes() {
    let schema: Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/web-scorecard.schema.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let scorecard: WebScorecard = serde_json::from_str(&first_golden()).unwrap();
    let emitted: Value = serde_json::to_value(&scorecard).unwrap();

    let required: BTreeSet<String> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().into())
        .collect();
    assert_eq!(keys(&emitted), keys(&schema["properties"]));
    assert_eq!(
        keys(&emitted),
        required,
        "every top-level field is required"
    );
    assert_eq!(schema["additionalProperties"], Value::Bool(false));

    let row_schema = &schema["$defs"]["ResultRow"];
    let row_required: BTreeSet<String> = row_schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().into())
        .collect();
    let row_properties = keys(&row_schema["properties"]);
    for row in emitted["results"].as_array().unwrap() {
        let row_keys = keys(row);
        assert!(
            row_required.is_subset(&row_keys),
            "row lacks a required key: {row}"
        );
        assert!(
            row_keys.is_subset(&row_properties),
            "row carries an undescribed key: {row}"
        );
    }
    let statuses: BTreeSet<String> = ScorecardStatus::ALL
        .iter()
        .map(|s| s.as_str().to_string())
        .collect();
    let schema_statuses: BTreeSet<String> = row_schema["properties"]["status"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().into())
        .collect();
    assert_eq!(statuses, schema_statuses);

    for (def, sample) in [
        ("StatusTally", &emitted["summary"]),
        ("CoverageSummary", &emitted["coverage_summary"]),
        ("Score", &emitted["score"]),
        ("ToolIdentity", &emitted["tool"]),
        ("CategoryRollup", &emitted["categories"][0]),
    ] {
        assert_eq!(
            keys(sample),
            keys(&schema["$defs"][def]["properties"]),
            "{def}"
        );
    }
}

#[test]
fn keywords_and_tiers_serialize_under_their_registry_spelling() {
    assert_eq!(serde_json::to_value(WebCheckKeyword::Must).unwrap(), "must");
    assert_eq!(
        serde_json::to_value(CHECKS[0].tier).unwrap(),
        CHECKS[0].tier.as_str()
    );
    assert_eq!(serde_json::to_value(ScorecardStatus::NA).unwrap(), "n_a");
    assert_eq!(
        serde_json::to_value(NaReason::PostureConsistent).unwrap(),
        "posture-consistent"
    );
}
