//! The two workspace files that are not TOML or JSON: the `packages`
//! sequence of a `pnpm-workspace.yaml`, read without a YAML parser, and the
//! `use` directives of a `go.work`.

/// The `packages` patterns of a `pnpm-workspace.yaml`. Only plain or quoted
/// scalars in a block or flow sequence are read; anything else is refused
/// rather than guessed at. Other top-level keys are skipped.
pub(super) fn pnpm_packages(text: &str) -> Result<Vec<String>, String> {
    let mut lines = text.lines().peekable();
    let mut packages = None;
    while let Some(line) = lines.next() {
        let content = strip_yaml_comment(line).trim_end();
        if content.trim().is_empty() || line.starts_with(char::is_whitespace) {
            continue;
        }
        let Some(rest) = content.strip_prefix("packages:") else {
            continue;
        };
        if packages.is_some() {
            return Err("`packages` appears twice".into());
        }
        let rest = rest.trim();
        let items = if rest.is_empty() {
            let mut items = Vec::new();
            while let Some(next) = lines.peek() {
                let item = strip_yaml_comment(next).trim();
                if item.is_empty() {
                    lines.next();
                    continue;
                }
                if !next.starts_with(char::is_whitespace) && !item.starts_with('-') {
                    break;
                }
                let Some(value) = item.strip_prefix('-') else {
                    return Err(format!("`{item}` is not a sequence entry"));
                };
                items.push(yaml_scalar(value.trim())?);
                lines.next();
            }
            items
        } else if let Some(inner) = rest.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            inner
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(yaml_scalar)
                .collect::<Result<_, _>>()?
        } else {
            return Err(format!("`packages: {rest}` is not a plain list"));
        };
        packages = Some(items);
    }
    Ok(packages.unwrap_or_default())
}

fn strip_yaml_comment(line: &str) -> &str {
    let mut quote = None;
    for (at, ch) in line.char_indices() {
        match (quote, ch) {
            (None, '\'' | '"') => quote = Some(ch),
            (Some(open), _) if ch == open => quote = None,
            (None, '#') if at == 0 || line[..at].ends_with(char::is_whitespace) => {
                return &line[..at];
            }
            _ => {}
        }
    }
    line
}

/// A quoted or plain YAML scalar, refusing tags, anchors, aliases, and
/// nested collections.
fn yaml_scalar(value: &str) -> Result<String, String> {
    let quoted = |q: char| value.len() >= 2 && value.starts_with(q) && value.ends_with(q);
    if quoted('\'') {
        return Ok(value[1..value.len() - 1].replace("''", "'"));
    }
    if quoted('"') {
        let inner = &value[1..value.len() - 1];
        if inner.contains('\\') {
            return Err(format!("escape sequences in `{value}`"));
        }
        return Ok(inner.to_string());
    }
    let structural = value.is_empty()
        || value.starts_with(['!', '&', '*', '[', '{', '|', '>', '-', '\'', '"'])
        || value.contains(": ");
    if structural {
        return Err(format!("`{value}` is not a plain string"));
    }
    Ok(value.to_string())
}

/// The directories a `go.work` file's `use` directives name.
pub(super) fn go_work_uses(text: &str) -> Vec<String> {
    let mut uses = Vec::new();
    let mut in_block = false;
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or_default().trim();
        if in_block {
            if line == ")" {
                in_block = false;
            } else if !line.is_empty() {
                uses.push(unquote_go(line));
            }
            continue;
        }
        let Some(rest) = line.strip_prefix("use") else {
            continue;
        };
        let rest = rest.trim();
        if rest == "(" {
            in_block = true;
        } else if let Some(inner) = rest.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
            uses.extend(inner.split_whitespace().map(unquote_go));
        } else if !rest.is_empty() && line.starts_with("use ") {
            uses.push(unquote_go(rest));
        }
    }
    uses
}

fn unquote_go(path: &str) -> String {
    path.trim_matches(|c| c == '"' || c == '`').to_string()
}
