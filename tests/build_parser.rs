//! Integration tests for `build_support/parser.rs`.
//!
//! Test-first per the spec-vendor plan U3: every error mode is asserted with
//! a fixture string, and the parser is implemented to satisfy those assertions.
//! The build script (`build.rs`) and this test driver both pull the parser in
//! via `#[path]` so the same code is exercised by `cargo test` and `cargo build`.

#[path = "../build_support/parser.rs"]
mod parser;

use parser::{
    Antecedent, Applicability, Level, ParseError, ParsedRequirement, aggregate, emit_rust,
    parse_principle_file,
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
        Applicability::Conditional {
            condition: Some("condition holds".to_string()),
            antecedent: None,
        }
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
            applicability: Applicability::Conditional {
                condition: Some("auth flow".into()),
                antecedent: None,
            },
        },
        ParsedRequirement {
            id: "p1-must-baz".into(),
            principle: 1,
            level: Level::Must,
            summary: "Conditional with audit_id antecedent.".into(),
            applicability: Applicability::Conditional {
                condition: None,
                antecedent: Some(Antecedent {
                    audit_id: "p1-prereq".into(),
                }),
            },
        },
    ];
    let src = emit_rust(&reqs, "0.2.0");

    assert!(src.contains("pub static REQUIREMENTS"));
    assert!(src.contains(r#"id: "p1-must-foo""#));
    assert!(src.contains(r#"id: "p1-must-bar""#));
    assert!(src.contains("Level::Must"));
    assert!(src.contains("Applicability::Universal"));
    assert!(src.contains(r#"condition: Some("auth flow")"#));
    assert!(src.contains(r#"audit_id: "p1-prereq""#));
    assert!(
        src.contains(r#"Quotes \"inside\" and \\ backslash."#),
        "summary must escape quotes and backslashes for Rust string literal"
    );
    assert!(src.contains(r#"pub const SPEC_VERSION: &str = "0.2.0";"#));
}

#[test]
fn parses_new_conditional_antecedent_shape() {
    let src = r#"---
id: p2
title: Conditional audit_id shape
last-revised: 2026-01-01
status: draft
requirements:
  - id: p2-must-schema-when-json
    level: must
    applicability:
      kind: conditional
      antecedent:
        audit_id: p2-json-output
    summary: If --output json is supported, the schema must be discoverable.
---
"#;
    let parsed = parse_principle_file("p2-cond.md", src).expect("valid input parses");
    assert_eq!(parsed.len(), 1);
    assert_eq!(
        parsed[0].applicability,
        Applicability::Conditional {
            condition: None,
            antecedent: Some(Antecedent {
                audit_id: "p2-json-output".to_string(),
            }),
        }
    );
}

#[test]
fn rejects_unknown_kind_value() {
    let src = r#"---
id: p2
title: Bad kind
last-revised: 2026-01-01
status: draft
requirements:
  - id: p2-must-foo
    level: must
    applicability:
      kind: optional
      antecedent:
        audit_id: p2-something
    summary: Bad kind.
---
"#;
    let err = parse_principle_file("p2-bad.md", src).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("kind: optional"),
        "should cite the bad kind: {msg}"
    );
}

#[test]
fn rejects_conditional_kind_with_no_condition_or_antecedent() {
    let src = r#"---
id: p2
title: Empty conditional
last-revised: 2026-01-01
status: draft
requirements:
  - id: p2-must-foo
    level: must
    applicability:
      kind: conditional
    summary: Bare conditional.
---
"#;
    let err = parse_principle_file("p2-bad.md", src).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("p2-must-foo"));
    assert!(msg.contains("conditional"));
}

#[test]
fn rejects_antecedent_missing_audit_id() {
    let src = r#"---
id: p2
title: Bad antecedent
last-revised: 2026-01-01
status: draft
requirements:
  - id: p2-must-foo
    level: must
    applicability:
      kind: conditional
      antecedent:
        kind: bundle-present
    summary: antecedent without audit_id.
---
"#;
    let err = parse_principle_file("p2-bad.md", src).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("audit_id"),
        "should hint audit_id is required: {msg}"
    );
}

#[test]
fn vendored_spec_parses_to_expected_requirement_count() {
    // Drives the same content build.rs will see. This asserts the parser
    // remains consistent with the real spec currently vendored under
    // `src/principles/spec/`. Bumping the count is a deliberate act tied to
    // a spec sync — the test deliberately fails until the new total is
    // acknowledged here, mirroring `registry_size_matches_spec` in
    // `src/principles/registry.rs`.
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

    let combined = aggregate(parsed_per_file).expect("no duplicates in vendored spec");
    assert_eq!(combined.len(), 59, "vendored spec ships 59 requirements");

    // First entry should still be p1-must-env-var — the order is filename-
    // sorted then spec-frontmatter-order, and v0.4.0 only appended new IDs.
    assert_eq!(combined[0].id, "p1-must-env-var");
    // Last entry is now from the new P8 principle (last MAY in
    // p8-discoverable-skill-bundle.md).
    assert_eq!(combined.last().unwrap().id, "p8-may-bundle-update");
}

// ─────────────────────────────────────────────────────────────────────────
// Red team: adversarial YAML inputs that try to slip past the parser.
// Every entry below tests a footgun a future spec author could fall into,
// or a subtle malformed value that the parser must surface loudly.
// ─────────────────────────────────────────────────────────────────────────

const RT_FIXTURE_HEAD: &str = r#"---
id: p2
title: Red team
last-revised: 2026-01-01
status: draft
requirements:
"#;

fn rt_assert_error_mentions(content: &str, expected_substrings: &[&str]) {
    // Append the closing frontmatter fence so each fixture is a complete
    // markdown file. The body is intentionally empty — every fixture
    // exercises the requirement-parsing path, not body content.
    let full = format!("{content}---\n");
    let err = parse_principle_file("p2-rt.md", &full).unwrap_err();
    let msg = format!("{err}");
    for s in expected_substrings {
        assert!(
            msg.contains(s),
            "error message must contain {s:?}, got: {msg}"
        );
    }
}

#[test]
fn rt_rejects_whitespace_only_audit_id() {
    // Subtle footgun — `is_empty` catches "" but not "   ". The parser must
    // trim before deciding "non-empty".
    let yaml = format!(
        "{RT_FIXTURE_HEAD}\
         \x20\x20- id: p2-must-x\n\
         \x20\x20\x20\x20level: must\n\
         \x20\x20\x20\x20applicability:\n\
         \x20\x20\x20\x20\x20\x20kind: conditional\n\
         \x20\x20\x20\x20\x20\x20antecedent:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20audit_id: '   '\n\
         \x20\x20\x20\x20summary: bad audit_id\n"
    );
    rt_assert_error_mentions(&yaml, &["p2-must-x", "audit_id", "non-empty"]);
}

#[test]
fn rt_rejects_antecedent_as_string() {
    // YAML author wrote `antecedent: p2-json-output` instead of the mapping
    // form. The parser must surface this with a hint at the expected shape.
    let yaml = format!(
        "{RT_FIXTURE_HEAD}\
         \x20\x20- id: p2-must-x\n\
         \x20\x20\x20\x20level: must\n\
         \x20\x20\x20\x20applicability:\n\
         \x20\x20\x20\x20\x20\x20kind: conditional\n\
         \x20\x20\x20\x20\x20\x20antecedent: p2-json-output\n\
         \x20\x20\x20\x20summary: antecedent-as-string\n"
    );
    rt_assert_error_mentions(&yaml, &["p2-must-x", "antecedent", "mapping"]);
}

#[test]
fn rt_rejects_antecedent_as_list() {
    // Compound antecedents (`all_of` / `any_of`) are deferred to v2 per plan
    // Sub-decision 2b. v1 must reject the list form.
    let yaml = format!(
        "{RT_FIXTURE_HEAD}\
         \x20\x20- id: p2-must-x\n\
         \x20\x20\x20\x20level: must\n\
         \x20\x20\x20\x20applicability:\n\
         \x20\x20\x20\x20\x20\x20kind: conditional\n\
         \x20\x20\x20\x20\x20\x20antecedent:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20- audit_id: p2-json-output\n\
         \x20\x20\x20\x20\x20\x20\x20\x20- audit_id: p2-yaml-output\n\
         \x20\x20\x20\x20summary: antecedent-as-list\n"
    );
    rt_assert_error_mentions(&yaml, &["p2-must-x", "antecedent", "mapping"]);
}

#[test]
fn rt_rejects_kind_with_wrong_case() {
    // YAML is case-sensitive; "Conditional" is not "conditional". An author
    // copying from a markdown example might capitalize and silently get a
    // different code path.
    let yaml = format!(
        "{RT_FIXTURE_HEAD}\
         \x20\x20- id: p2-must-x\n\
         \x20\x20\x20\x20level: must\n\
         \x20\x20\x20\x20applicability:\n\
         \x20\x20\x20\x20\x20\x20kind: Conditional\n\
         \x20\x20\x20\x20\x20\x20antecedent:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20audit_id: p2-json-output\n\
         \x20\x20\x20\x20summary: capitalized kind\n"
    );
    rt_assert_error_mentions(&yaml, &["p2-must-x", "Conditional"]);
}

#[test]
fn rt_rejects_mixed_legacy_if_and_new_kind() {
    // Author mid-migration writes both shapes in the same row. The legacy
    // `if:` branch only fires for `map.len() == 1`, so today both keys would
    // silently pick the new shape and drop the legacy prose. The parser
    // rejects the mixed shape outright.
    let yaml = format!(
        "{RT_FIXTURE_HEAD}\
         \x20\x20- id: p2-must-x\n\
         \x20\x20\x20\x20level: must\n\
         \x20\x20\x20\x20applicability:\n\
         \x20\x20\x20\x20\x20\x20if: CLI emits structured output\n\
         \x20\x20\x20\x20\x20\x20kind: conditional\n\
         \x20\x20\x20\x20\x20\x20antecedent:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20audit_id: p2-json-output\n\
         \x20\x20\x20\x20summary: mixed legacy+new shape\n"
    );
    rt_assert_error_mentions(&yaml, &["p2-must-x", "if:", "kind:"]);
}

#[test]
fn rt_rejects_antecedent_with_compound_op_key() {
    // The v1 schema is strict about `antecedent` contents: only `audit_id`
    // is permitted. A v2-style `op: any_of` accidentally added to a v1 row
    // must error so v2 syntax doesn't ship under v1 semantics.
    let yaml = format!(
        "{RT_FIXTURE_HEAD}\
         \x20\x20- id: p2-must-x\n\
         \x20\x20\x20\x20level: must\n\
         \x20\x20\x20\x20applicability:\n\
         \x20\x20\x20\x20\x20\x20kind: conditional\n\
         \x20\x20\x20\x20\x20\x20antecedent:\n\
         \x20\x20\x20\x20\x20\x20\x20\x20audit_id: p2-json-output\n\
         \x20\x20\x20\x20\x20\x20\x20\x20op: any_of\n\
         \x20\x20\x20\x20summary: smuggled v2 syntax\n"
    );
    rt_assert_error_mentions(&yaml, &["p2-must-x", "op", "v1 schema"]);
}

#[test]
fn rt_rejects_antecedent_null() {
    // `antecedent: null` slipped past an earlier draft; the parser must
    // treat it the same as a non-mapping value and refuse the row.
    let yaml = format!(
        "{RT_FIXTURE_HEAD}\
         \x20\x20- id: p2-must-x\n\
         \x20\x20\x20\x20level: must\n\
         \x20\x20\x20\x20applicability:\n\
         \x20\x20\x20\x20\x20\x20kind: conditional\n\
         \x20\x20\x20\x20\x20\x20antecedent: null\n\
         \x20\x20\x20\x20summary: null antecedent\n"
    );
    rt_assert_error_mentions(&yaml, &["p2-must-x", "antecedent", "mapping"]);
}

#[test]
fn rt_rejects_empty_antecedent_mapping() {
    // `antecedent: {}` is a mapping with no `audit_id`. Surface the missing
    // field by name.
    let yaml = format!(
        "{RT_FIXTURE_HEAD}\
         \x20\x20- id: p2-must-x\n\
         \x20\x20\x20\x20level: must\n\
         \x20\x20\x20\x20applicability:\n\
         \x20\x20\x20\x20\x20\x20kind: conditional\n\
         \x20\x20\x20\x20\x20\x20antecedent: {{}}\n\
         \x20\x20\x20\x20summary: empty antecedent\n"
    );
    rt_assert_error_mentions(&yaml, &["p2-must-x", "audit_id"]);
}

#[test]
fn rt_emit_rust_escapes_quotes_in_antecedent_audit_id() {
    // Defense-in-depth: the parser already rejects whitespace-only
    // audit_ids, but emit_rust should still escape any string content
    // that ends up in the generated source. A audit_id containing a
    // quote (impossible today but cheap to guard against) must not break
    // the generated Rust literal.
    let reqs = vec![ParsedRequirement {
        id: "p2-must-quoted".into(),
        principle: 2,
        level: Level::Must,
        summary: "row with a hostile audit_id".into(),
        applicability: Applicability::Conditional {
            condition: None,
            antecedent: Some(Antecedent {
                audit_id: r#"p2-"injected""#.into(),
            }),
        },
    }];
    let src = emit_rust(&reqs, "0.0.0");
    // The audit_id literal must escape the embedded quotes; the generated
    // line should parse as valid Rust.
    assert!(
        src.contains(r#"audit_id: "p2-\"injected\"""#),
        "generated source must escape quotes in audit_id, got:\n{src}",
    );
}
