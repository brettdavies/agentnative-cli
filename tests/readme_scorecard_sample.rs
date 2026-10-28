//! Holds the README's sample scorecard to the build.
//!
//! The sample under `## JSON Output` is copied by hand from a real run, so
//! nothing regenerates it when the scorecard shape or the vendored spec
//! moves. This fails when the section's `schema_version` or `spec_version`
//! trails the build, or when an object in the sample names a property the
//! schema does not declare or leaves out one the schema requires.

use agentnative::principles::registry::SPEC_VERSION;
use agentnative::scorecard::SCHEMA_VERSION;
use serde_json::Value;

const SECTION_HEADING: &str = "\n## JSON Output\n";

fn json_output_section() -> String {
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md"))
        .expect("README.md is readable");
    let start = readme
        .find(SECTION_HEADING)
        .expect("README.md has a `## JSON Output` section")
        + SECTION_HEADING.len();
    let rest = &readme[start..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    rest[..end].to_string()
}

fn sample(section: &str) -> Value {
    let open = "```json\n";
    let start = section
        .find(open)
        .expect("the JSON Output section has a ```json sample")
        + open.len();
    let end = section[start..]
        .find("\n```")
        .expect("the ```json sample is closed");
    serde_json::from_str(&section[start..start + end]).expect("the README sample is valid JSON")
}

fn resolve<'a>(schema: &'a Value, node: &'a Value) -> &'a Value {
    match node["$ref"].as_str() {
        Some(reference) => {
            let name = reference
                .strip_prefix("#/$defs/")
                .expect("the schema refers only to its own $defs");
            &schema["$defs"][name]
        }
        None => node,
    }
}

/// Collects every property-name disagreement between `value` and the schema
/// node it sits under, at any depth.
fn shape_errors(schema: &Value, node: &Value, value: &Value, path: &str, errors: &mut Vec<String>) {
    let node = resolve(schema, node);
    match value {
        Value::Object(fields) => {
            let Some(properties) = node["properties"].as_object() else {
                return;
            };
            for required in node["required"].as_array().into_iter().flatten() {
                let name = required.as_str().expect("required names are strings");
                if !fields.contains_key(name) {
                    errors.push(format!("{path}: missing required `{name}`"));
                }
            }
            for (name, field) in fields {
                match properties.get(name) {
                    Some(child) => {
                        shape_errors(schema, child, field, &format!("{path}.{name}"), errors)
                    }
                    None => errors.push(format!("{path}: `{name}` is not in the schema")),
                }
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                shape_errors(
                    schema,
                    &node["items"],
                    item,
                    &format!("{path}[{index}]"),
                    errors,
                );
            }
        }
        _ => {}
    }
}

#[test]
fn readme_sample_scorecard_reports_this_builds_versions() {
    let section = json_output_section();
    let prose = format!("`schema_version: \"{SCHEMA_VERSION}\"`");
    assert!(
        section.contains(&prose),
        "the JSON Output prose should name {prose}"
    );

    let sample = sample(&section);
    assert_eq!(sample["schema_version"], SCHEMA_VERSION);
    assert_eq!(sample["spec_version"], SPEC_VERSION);
}

#[test]
fn readme_sample_scorecard_matches_the_schema() {
    let schema: Value = serde_json::from_str(include_str!("../schema/scorecard.schema.json"))
        .expect("the scorecard schema is valid JSON");
    let mut errors = Vec::new();
    shape_errors(
        &schema,
        &schema,
        &sample(&json_output_section()),
        "$",
        &mut errors,
    );
    assert!(
        errors.is_empty(),
        "README sample disagrees with schema/scorecard.schema.json:\n{}",
        errors.join("\n")
    );
}
