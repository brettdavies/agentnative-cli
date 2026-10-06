//! Fixture for the `code-unwrap` audit's macro-interior matcher.
//!
//! Line numbers here are pinned by `tests/integration.rs`; append new cases at
//! the end rather than inserting above an existing one.

use std::fmt::Write as _;

pub fn expression_argument(out: &mut String, v: Option<u8>) {
    write!(out, "{}", v.unwrap()).ok();
}

// The call must stay on its own line after the macro's opening line: that is
// what exercises the line offset and the column rule that applies only to the
// interior's first line. rustfmt would otherwise collapse it.
#[rustfmt::skip]
pub fn multi_line_argument(out: &mut String, v: Option<u8>) {
    write!(
        out,
        "{}",
        v.unwrap()
    )
    .ok();
}

pub fn nested_macro(out: &mut String, v: Option<u8>) {
    write!(out, "{}", format!("{}", v.unwrap())).ok();
}

thread_local! {
    pub static PROBE: u8 = load().unwrap();
}

pub fn string_literal_only() {
    println!("never call .unwrap() in production");
}

pub fn raw_string_only() {
    println!("{}", r#"never call .unwrap() here either"#);
}

pub fn commented_interior(v: Option<u8>) {
    println!(
        // never write v.unwrap() in this macro
        "{}",
        v.unwrap_or(0)
    );
}

pub fn sibling_method_names(v: Option<u8>, r: Result<u8, u8>, f: fn() -> u8) {
    println!("{}", v.unwrap_or(0));
    println!("{}", v.unwrap_or_else(f));
    println!("{}", r.unwrap_err());
    println!("{}", v.expect("m"));
}

macro_rules! fragment_shape {
    ($x:expr) => {
        $x.unwrap()
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn gated() {
        let v: Option<u8> = Some(1);
        assert_eq!(v.unwrap(), 1);
    }
}

fn load() -> Option<u8> {
    Some(1)
}

// Token-only macros: their arguments are tokens, not code this crate runs, so
// no call in them may be reported.
pub fn token_only_macros(v: Option<u8>) {
    let _ = stringify!(v.unwrap());
    let _ = concat!("a", stringify!(v.unwrap()));
}

// A macro_rules! transcriber nested in another macro's arguments is a template,
// not code, and tree-sitter builds no `macro_definition` node for it.
cfg_if! {
    if #[cfg(unix)] {
        macro_rules! nested_template {
            () => { GLOBAL.unwrap() };
        }
    }
}
