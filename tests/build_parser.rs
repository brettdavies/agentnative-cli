//! Integration tests for `build_support/parser.rs`.
//!
//! Test-first per the spec-vendor plan U3: every error mode is asserted with
//! a fixture string, and the parser is implemented to satisfy those assertions.
//! The build script (`build.rs`) and this test driver both pull the parser in
//! via `#[path]` so the same code is exercised by `cargo test` and `cargo build`.

#[path = "../build_support/parser.rs"]
mod parser;

use parser::{
    Applicability, Level, ParseError, ParsedRequirement, aggregate, emit_rust, parse_principle_file,
};

const VALID_P1: &str = r#"---
id: p1
title: Test Principle
last-revised: 2026-01-01
status: draft
requirements:
  - id: p1-must-foo
    level: must
    applicability: universal
    summary: Must do foo.
  - id: p1-must-bar
    level: must
    applicability:
      if: condition holds
    summary: Must do bar when condition holds.
  - id: p1-should-baz
    level: should
    applicability: universal
    summary: Should do baz.
  - id: p1-may-qux
    level: may
    applicability: universal
    summary: May do qux.
---

# Body

Markdown body content.
"#;

const VALID_P2: &str = r#"---
id: p2
title: Second Principle
last-revised: 2026-01-01
status: draft
requirements:
  - id: p2-must-alpha
    level: must
    applicability: universal
    summary: Must alpha.
  - id: p2-may-beta
    level: may
    applicability: universal
    summary: May beta.
---
"#;

#[test]
fn parses_valid_principle_file_in_source_order() {
    let parsed = parse_principle_file("p1-test.md", VALID_P1).expect("valid input parses");
    assert_eq!(parsed.len(), 4);

    assert_eq!(parsed[0].id, "p1-must-foo");
    assert_eq!(parsed[0].principle, 1);
    assert_eq!(parsed[0].level, Level::Must);
    assert_eq!(parsed[0].applicability, Applicability::Universal);
    assert_eq!(parsed[0].summary, "Must do foo.");

    assert_eq!(parsed[1].id, "p1-must-bar");
    assert_eq!(
        parsed[1].applicability,
        Applicability::Conditional("condition holds".to_string())
    );

    assert_eq!(parsed[2].id, "p1-should-baz");
    assert_eq!(parsed[2].level, Level::Should);

    assert_eq!(parsed[3].id, "p1-may-qux");
    assert_eq!(parsed[3].level, Level::May);
}

#[test]
fn aggregate_sorts_by_principle_then_level_preserving_source_order() {
    let p1 = parse_principle_file("p1-test.md", VALID_P1).unwrap();
    let p2 = parse_principle_file("p2-test.md", VALID_P2).unwrap();

    let combined = aggregate(vec![
        ("p2-test.md".to_string(), p2.clone()),
        ("p1-test.md".to_string(), p1.clone()),
    ])
    .expect("no duplicates, no errors");

    let ids: Vec<&str> = combined.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            // p1: musts in source order, then should, then may
            "p1-must-foo",
            "p1-must-bar",
            "p1-should-baz",
            "p1-may-qux",
            // p2: must, then may
            "p2-must-alpha",
            "p2-may-beta",
        ],
        "sort: (principle, level: must<should<may, source-order)"
    );
}

#[test]
fn duplicate_id_across_files_errors_with_both_paths() {
    let dup = r#"---
id: p1
title: Dup
last-revised: 2026-01-01
status: draft
requirements:
  - id: p1-must-foo
    level: must
    applicability: universal
    summary: Same ID as VALID_P1's first entry.
---
"#;
    let p1 = parse_principle_file("p1-test.md", VALID_P1).unwrap();
    let p1_dup = parse_principle_file("p1-dup.md", dup).unwrap();

    let err = aggregate(vec![
        ("p1-test.md".to_string(), p1),
        ("p1-dup.md".to_string(), p1_dup),
    ])
    .unwrap_err();

    match err {
        ParseError::DuplicateId { id, file_a, file_b } => {
            assert_eq!(id, "p1-must-foo");
            // Order: first-seen file_a, second-seen file_b
            assert_eq!(file_a, "p1-test.md");
            assert_eq!(file_b, "p1-dup.md");
        }
        other => panic!("expected DuplicateId, got {other:?}"),
    }
}

#[test]
fn missing_summary_errors_with_field_name_and_requirement_id() {
    let bad = r#"---
id: p1
title: Bad
last-revised: 2026-01-01
status: draft
requirements:
  - id: p1-must-foo
    level: must
    applicability: universal
---
"#;
    let err = parse_principle_file("p1-bad.md", bad).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("p1-bad.md"), "must cite file path: {msg}");
    assert!(msg.contains("summary"), "must cite missing field: {msg}");
}

#[test]
fn unknown_level_errors_with_allowed_values() {
    let bad = r#"---
id: p1
title: Bad
last-revised: 2026-01-01
status: draft
requirements:
  - id: p1-foo
    level: recommended
    applicability: universal
    summary: Bad level.
---
"#;
    let err = parse_principle_file("p1-bad.md", bad).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("p1-bad.md"));
    assert!(msg.contains("recommended"));
    assert!(msg.contains("must"), "should hint allowed values: {msg}");
}

#[test]
fn unknown_applicability_shape_errors_with_hint() {
    let bad = r#"---
id: p1
title: Bad
last-revised: 2026-01-01
status: draft
requirements:
  - id: p1-must-foo
    level: must
    applicability:
      unless: condition fails
    summary: Bad shape.
---
"#;
    let err = parse_principle_file("p1-bad.md", bad).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("p1-bad.md"));
    assert!(msg.contains("p1-must-foo"));
    assert!(msg.contains("applicability"));
}

#[test]
fn invalid_universal_string_errors() {
    let bad = r#"---
id: p1
title: Bad
last-revised: 2026-01-01
status: draft
requirements:
  - id: p1-must-foo
    level: must
    applicability: cosmic
    summary: Bad universal value.
---
"#;
    let err = parse_principle_file("p1-bad.md", bad).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("p1-bad.md"));
    assert!(msg.contains("cosmic"));
    assert!(
        msg.contains("universal"),
        "should mention the only accepted bare-string value: {msg}"
    );
}

#[test]
fn unterminated_frontmatter_errors() {
    let bad = "---\nid: p1\ntitle: Unterminated\n# never closed\n";
    let err = parse_principle_file("p1-bad.md", bad).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("p1-bad.md"));
    assert!(
        msg.contains("frontmatter") || msg.contains("---"),
        "should mention frontmatter: {msg}"
    );
}

#[test]
fn invalid_principle_id_errors() {
    let bad = r#"---
id: not-a-principle
title: Bad
last-revised: 2026-01-01
status: draft
requirements:
  - id: foo
    level: must
    applicability: universal
    summary: Whatever.
---
"#;
    let err = parse_principle_file("not-p.md", bad).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("not-a-principle"));
}

#[test]
fn empty_requirements_errors() {
    let bad = r#"---
id: p1
title: Empty
last-revised: 2026-01-01
status: draft
requirements: []
---
"#;
    let err = parse_principle_file("p1-empty.md", bad).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("p1-empty.md"));
}

#[test]
fn emit_rust_produces_well_formed_source() {
    let reqs = vec![
        ParsedRequirement {
            id: "p1-must-foo".into(),
            principle: 1,
            level: Level::Must,
            summary: "Must foo.".into(),
            applicability: Applicability::Universal,
        },
        ParsedRequirement {
            id: "p1-must-bar".into(),
            principle: 1,
            level: Level::Must,
            summary: r#"Quotes "inside" and \ backslash."#.into(),
            applicability: Applicability::Conditional("auth flow".into()),
        },
    ];
    let src = emit_rust(&reqs, "0.2.0");

    assert!(src.contains("pub static REQUIREMENTS"));
    assert!(src.contains(r#"id: "p1-must-foo""#));
    assert!(src.contains(r#"id: "p1-must-bar""#));
    assert!(src.contains("Level::Must"));
    assert!(src.contains("Applicability::Universal"));
    assert!(src.contains(r#"Applicability::Conditional("auth flow")"#));
    assert!(
        src.contains(r#"Quotes \"inside\" and \\ backslash."#),
        "summary must escape quotes and backslashes for Rust string literal"
    );
    assert!(src.contains(r#"pub const SPEC_VERSION: &str = "0.2.0";"#));
}

#[test]
fn vendored_v0_2_0_parses_to_46_requirements() {
    // Drives the same content build.rs will see. This asserts the parser
    // remains consistent with the real spec we're shipping at v0.2.0.
    use std::fs;

    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/principles/spec/principles");
    let mut files: Vec<_> = fs::read_dir(&dir)
        .expect("vendored principles dir exists")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|x| x == "md")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with('p'))
        })
        .collect();
    files.sort();

    let mut parsed_per_file = Vec::new();
    for path in &files {
        let content = fs::read_to_string(path).unwrap();
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        let reqs = parse_principle_file(&name, &content)
            .unwrap_or_else(|e| panic!("parse {name} failed: {e}"));
        parsed_per_file.push((name, reqs));
    }

    let combined = aggregate(parsed_per_file).expect("no duplicates in v0.2.0");
    assert_eq!(combined.len(), 46, "v0.2.0 ships 46 requirements");

    // First entry should be p1-must-env-var (matches existing hand-maintained order).
    assert_eq!(combined[0].id, "p1-must-env-var");
    // Last entry should be p7-may-auto-verbosity.
    assert_eq!(combined.last().unwrap().id, "p7-may-auto-verbosity");
}
