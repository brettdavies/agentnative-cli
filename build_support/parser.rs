//! Spec frontmatter parser used by `build.rs` to generate `REQUIREMENTS`.
//!
//! Mirrored by `agentnative:scripts/validate-principles.mjs` on the spec side
//! — the two parsers must agree on the schema. If a vendored file fails here,
//! the build fails loudly: every error names the offending file, requirement
//! id, and field so a human can fix it without grepping.

use std::collections::HashMap;
use std::fmt;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Must,
    Should,
    May,
}

impl Level {
    fn sort_key(self) -> u8 {
        match self {
            Level::Must => 0,
            Level::Should => 1,
            Level::May => 2,
        }
    }

    fn rust_variant(self) -> &'static str {
        match self {
            Level::Must => "Level::Must",
            Level::Should => "Level::Should",
            Level::May => "Level::May",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applicability {
    Universal,
    Conditional(String),
}

#[derive(Debug, Clone)]
pub struct ParsedRequirement {
    pub id: String,
    pub principle: u8,
    pub level: Level,
    pub summary: String,
    pub applicability: Applicability,
}

#[derive(Debug)]
pub enum ParseError {
    UnterminatedFrontmatter {
        file: String,
    },
    YamlError {
        file: String,
        message: String,
    },
    InvalidPrincipleId {
        file: String,
        value: String,
    },
    DuplicateId {
        id: String,
        file_a: String,
        file_b: String,
    },
    UnknownLevel {
        file: String,
        requirement_id: String,
        value: String,
    },
    UnknownApplicability {
        file: String,
        requirement_id: String,
        hint: String,
    },
    MissingField {
        file: String,
        requirement_id: Option<String>,
        field: String,
    },
    InvalidUniversal {
        file: String,
        requirement_id: String,
        value: String,
    },
    EmptyRequirements {
        file: String,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::UnterminatedFrontmatter { file } => write!(
                f,
                "{file}: frontmatter not terminated — expected closing `---` line"
            ),
            ParseError::YamlError { file, message } => {
                write!(f, "{file}: YAML parse error: {message}")
            }
            ParseError::InvalidPrincipleId { file, value } => write!(
                f,
                "{file}: file-level `id` must be `pN` (e.g., `p1`), got `{value}`"
            ),
            ParseError::DuplicateId { id, file_a, file_b } => write!(
                f,
                "duplicate requirement id `{id}` in `{file_a}` and `{file_b}` — ids must be unique across all principles"
            ),
            ParseError::UnknownLevel {
                file,
                requirement_id,
                value,
            } => write!(
                f,
                "{file}: requirement `{requirement_id}` has unknown level `{value}` — must be one of `must`, `should`, `may`"
            ),
            ParseError::UnknownApplicability {
                file,
                requirement_id,
                hint,
            } => write!(
                f,
                "{file}: requirement `{requirement_id}` has unsupported `applicability` shape — {hint}"
            ),
            ParseError::MissingField {
                file,
                requirement_id,
                field,
            } => match requirement_id {
                Some(id) => write!(
                    f,
                    "{file}: requirement `{id}` is missing required field `{field}`"
                ),
                None => write!(f, "{file}: missing required top-level field `{field}`"),
            },
            ParseError::InvalidUniversal {
                file,
                requirement_id,
                value,
            } => write!(
                f,
                "{file}: requirement `{requirement_id}` has bare-string `applicability: {value}` — only `universal` is accepted as a bare string; conditional forms must use `applicability: {{ if: \"<prose>\" }}`"
            ),
            ParseError::EmptyRequirements { file } => {
                write!(f, "{file}: `requirements` list is empty")
            }
        }
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Deserialize)]
struct RawFrontmatter {
    id: String,
    requirements: Vec<RawRequirement>,
    // Fields we don't need at runtime: title, last-revised, status. Tolerated.
    #[serde(flatten)]
    _extra: serde_yaml::Mapping,
}

#[derive(Debug, Deserialize)]
struct RawRequirement {
    id: Option<String>,
    level: Option<serde_yaml::Value>,
    applicability: Option<serde_yaml::Value>,
    summary: Option<String>,
    #[serde(flatten)]
    _extra: serde_yaml::Mapping,
}

/// Parse a single principle markdown file.
///
/// Returns the requirements declared in `requirements:` in source order.
pub fn parse_principle_file(
    file_path: &str,
    content: &str,
) -> Result<Vec<ParsedRequirement>, ParseError> {
    let yaml_block = extract_frontmatter(file_path, content)?;
    let raw: RawFrontmatter =
        serde_yaml::from_str(yaml_block).map_err(|e| ParseError::YamlError {
            file: file_path.to_string(),
            message: e.to_string(),
        })?;

    let principle = parse_principle_id(file_path, &raw.id)?;

    if raw.requirements.is_empty() {
        return Err(ParseError::EmptyRequirements {
            file: file_path.to_string(),
        });
    }

    let mut out = Vec::with_capacity(raw.requirements.len());
    for raw_req in raw.requirements {
        let id = raw_req.id.ok_or_else(|| ParseError::MissingField {
            file: file_path.to_string(),
            requirement_id: None,
            field: "id".into(),
        })?;

        let level = parse_level(file_path, &id, raw_req.level.as_ref())?;
        let summary = raw_req.summary.ok_or_else(|| ParseError::MissingField {
            file: file_path.to_string(),
            requirement_id: Some(id.clone()),
            field: "summary".into(),
        })?;
        let applicability = parse_applicability(file_path, &id, raw_req.applicability.as_ref())?;

        out.push(ParsedRequirement {
            id,
            principle,
            level,
            summary,
            applicability,
        });
    }

    Ok(out)
}

/// Aggregate per-file parse results into a single, sorted, deduped slice.
///
/// Sort order: `(principle, level: must<should<may, source-order-within-level)`.
/// Source order is preserved by stable-sorting on the composite key while
/// keeping the per-file source order as the secondary tiebreaker.
pub fn aggregate(
    parsed: Vec<(String, Vec<ParsedRequirement>)>,
) -> Result<Vec<ParsedRequirement>, ParseError> {
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut all: Vec<ParsedRequirement> = Vec::new();

    for (file, reqs) in parsed {
        for req in reqs {
            if let Some(existing_file) = seen.get(&req.id) {
                return Err(ParseError::DuplicateId {
                    id: req.id.clone(),
                    file_a: existing_file.clone(),
                    file_b: file.clone(),
                });
            }
            seen.insert(req.id.clone(), file.clone());
            all.push(req);
        }
    }

    // Stable sort preserves source order within identical (principle, level).
    all.sort_by_key(|r| (r.principle, r.level.sort_key()));
    Ok(all)
}

/// Emit Rust source for `$OUT_DIR/generated_requirements.rs`.
pub fn emit_rust(reqs: &[ParsedRequirement], spec_version: &str) -> String {
    let mut out = String::new();
    out.push_str(
        "// @generated by build.rs from src/principles/spec/principles/*.md.\n\
         // Do not edit by hand — rerun `cargo build` (or `scripts/sync-spec.sh` then `cargo build`)\n\
         // to regenerate.\n\n",
    );

    out.push_str("pub static REQUIREMENTS: &[Requirement] = &[\n");
    for r in reqs {
        out.push_str("    Requirement {\n");
        out.push_str(&format!("        id: \"{}\",\n", escape_rust_str(&r.id)));
        out.push_str(&format!("        principle: {},\n", r.principle));
        out.push_str(&format!("        level: {},\n", r.level.rust_variant()));
        out.push_str(&format!(
            "        summary: \"{}\",\n",
            escape_rust_str(&r.summary)
        ));
        match &r.applicability {
            Applicability::Universal => {
                out.push_str("        applicability: Applicability::Universal,\n");
            }
            Applicability::Conditional(cond) => {
                out.push_str(&format!(
                    "        applicability: Applicability::Conditional(\"{}\"),\n",
                    escape_rust_str(cond)
                ));
            }
        }
        out.push_str("    },\n");
    }
    out.push_str("];\n\n");

    out.push_str(&format!(
        "pub const SPEC_VERSION: &str = \"{}\";\n",
        escape_rust_str(spec_version)
    ));

    out
}

fn extract_frontmatter<'a>(file_path: &str, content: &'a str) -> Result<&'a str, ParseError> {
    // First non-empty line must be `---`. Then find next `---` on its own line.
    let trimmed = content.trim_start_matches('\u{feff}'); // tolerate BOM
    let after_first = trimmed
        .strip_prefix("---\n")
        .or_else(|| trimmed.strip_prefix("---\r\n"))
        .ok_or_else(|| ParseError::UnterminatedFrontmatter {
            file: file_path.to_string(),
        })?;

    let end_idx =
        find_closing_fence(after_first).ok_or_else(|| ParseError::UnterminatedFrontmatter {
            file: file_path.to_string(),
        })?;

    Ok(&after_first[..end_idx])
}

fn find_closing_fence(s: &str) -> Option<usize> {
    // Look for a line that is exactly `---` (possibly followed by \r and \n or EOF).
    let mut idx = 0;
    for line in s.split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
        if trimmed == "---" {
            return Some(idx);
        }
        idx += line.len();
    }
    None
}

fn parse_principle_id(file: &str, raw: &str) -> Result<u8, ParseError> {
    raw.strip_prefix('p')
        .and_then(|s| s.parse::<u8>().ok())
        .filter(|n| (1..=255).contains(n))
        .ok_or_else(|| ParseError::InvalidPrincipleId {
            file: file.to_string(),
            value: raw.to_string(),
        })
}

fn parse_level(
    file: &str,
    req_id: &str,
    value: Option<&serde_yaml::Value>,
) -> Result<Level, ParseError> {
    let value = value.ok_or_else(|| ParseError::MissingField {
        file: file.to_string(),
        requirement_id: Some(req_id.to_string()),
        field: "level".into(),
    })?;

    let s = value.as_str().ok_or_else(|| ParseError::UnknownLevel {
        file: file.to_string(),
        requirement_id: req_id.to_string(),
        value: format!("{value:?}"),
    })?;

    match s {
        "must" => Ok(Level::Must),
        "should" => Ok(Level::Should),
        "may" => Ok(Level::May),
        other => Err(ParseError::UnknownLevel {
            file: file.to_string(),
            requirement_id: req_id.to_string(),
            value: other.to_string(),
        }),
    }
}

fn parse_applicability(
    file: &str,
    req_id: &str,
    value: Option<&serde_yaml::Value>,
) -> Result<Applicability, ParseError> {
    let value = value.ok_or_else(|| ParseError::MissingField {
        file: file.to_string(),
        requirement_id: Some(req_id.to_string()),
        field: "applicability".into(),
    })?;

    if let Some(s) = value.as_str() {
        if s == "universal" {
            return Ok(Applicability::Universal);
        }
        return Err(ParseError::InvalidUniversal {
            file: file.to_string(),
            requirement_id: req_id.to_string(),
            value: s.to_string(),
        });
    }

    if let Some(map) = value.as_mapping() {
        let if_key = serde_yaml::Value::String("if".into());
        if map.len() == 1
            && let Some(if_val) = map.get(&if_key)
        {
            let cond = if_val
                .as_str()
                .ok_or_else(|| ParseError::UnknownApplicability {
                    file: file.to_string(),
                    requirement_id: req_id.to_string(),
                    hint: "`if:` value must be a non-empty string".into(),
                })?;
            if cond.is_empty() {
                return Err(ParseError::UnknownApplicability {
                    file: file.to_string(),
                    requirement_id: req_id.to_string(),
                    hint: "`if:` value must be a non-empty string".into(),
                });
            }
            return Ok(Applicability::Conditional(cond.to_string()));
        }
        return Err(ParseError::UnknownApplicability {
            file: file.to_string(),
            requirement_id: req_id.to_string(),
            hint: "expected `{ if: \"<prose>\" }`".into(),
        });
    }

    Err(ParseError::UnknownApplicability {
        file: file.to_string(),
        requirement_id: req_id.to_string(),
        hint: "must be `universal` or `{ if: \"<prose>\" }`".into(),
    })
}

fn escape_rust_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}
