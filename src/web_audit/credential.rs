//! The bearer token a run presents to the audited MCP endpoint, read from
//! `ANC_WEB_TOKEN` or one line of stdin, never from an argv value. The
//! value never prints: every type that can hold it redacts it from Debug.

use std::ffi::OsString;
use std::fmt;
use std::io::BufRead;
use std::sync::Arc;

/// The request header that carries a credential.
pub const AUTHORIZATION: &str = "Authorization";

/// What a credential's value prints as.
const REDACTED: &str = "[redacted]";

/// A bearer token for the audited MCP endpoint.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential(Arc<str>);

impl Credential {
    /// A bearer token, with surrounding whitespace trimmed. Every character
    /// left must be visible ASCII, which an HTTP header value carries as is.
    pub fn bearer(raw: &str, source: TokenSource) -> Result<Self, TokenError> {
        let token = raw.trim_matches(|c: char| c.is_ascii_whitespace());
        if token.is_empty() {
            return Err(TokenError::Empty(source));
        }
        if !token.chars().all(|c| c.is_ascii_graphic()) {
            return Err(TokenError::Invalid(source));
        }
        Ok(Credential(Arc::from(token)))
    }

    /// The `Authorization` header that presents this token.
    pub(crate) fn header(&self) -> (String, String) {
        (AUTHORIZATION.to_string(), format!("Bearer {}", self.0))
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Credential").field(&REDACTED).finish()
    }
}

/// Where a token was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenSource {
    /// The `ANC_WEB_TOKEN` environment variable.
    Env,
    /// One line of stdin, read for `--token-stdin`.
    Stdin,
}

/// Why no usable token was read. None of these carries the value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenError {
    /// The source held nothing but whitespace.
    Empty(TokenSource),
    /// The value holds whitespace, a control character, or non-ASCII.
    Invalid(TokenSource),
    /// `--token-stdin` was given while stdin is a terminal.
    StdinTerminal,
    /// Stdin could not be read.
    StdinUnreadable(String),
}

/// The run's token: one line of `stdin` when `from_stdin`, else `env`
/// (the value of `ANC_WEB_TOKEN`) when it is set. `None` when neither
/// presents one.
pub fn read_token(
    from_stdin: bool,
    env: Option<OsString>,
    stdin: &mut dyn BufRead,
    stdin_is_terminal: bool,
) -> Result<Option<Credential>, TokenError> {
    if from_stdin {
        // Reading a terminal would wait on a prompt nothing announced, and
        // echo the token as it was typed.
        if stdin_is_terminal {
            return Err(TokenError::StdinTerminal);
        }
        let mut line = Vec::new();
        stdin
            .read_until(b'\n', &mut line)
            .map_err(|e| TokenError::StdinUnreadable(e.kind().to_string()))?;
        let text = String::from_utf8(line).map_err(|_| TokenError::Invalid(TokenSource::Stdin))?;
        return Credential::bearer(&text, TokenSource::Stdin).map(Some);
    }
    let Some(value) = env else {
        return Ok(None);
    };
    let text = value
        .into_string()
        .map_err(|_| TokenError::Invalid(TokenSource::Env))?;
    Credential::bearer(&text, TokenSource::Env).map(Some)
}

/// Request headers as Debug prints them: a credential's value redacted.
pub struct RedactedHeaders<'a>(pub &'a [(String, String)]);

impl fmt::Debug for RedactedHeaders<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|(name, value)| {
                let shown = if name.eq_ignore_ascii_case(AUTHORIZATION) {
                    REDACTED
                } else {
                    value.as_str()
                };
                (name.as_str(), shown)
            }))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    const SECRET: &str = "s3cr3t-T0KEN";

    fn from_stdin(input: &str) -> Result<Option<Credential>, TokenError> {
        read_token(true, None, &mut Cursor::new(input.as_bytes()), false)
    }

    #[test]
    fn stdin_yields_its_first_line_trimmed() {
        let read = from_stdin(&format!("{SECRET}\r\nsecond line\n")).unwrap();
        assert_eq!(read, Some(Credential(Arc::from(SECRET))));
    }

    #[test]
    fn stdin_wins_over_the_environment() {
        let mut input = Cursor::new(format!("{SECRET}\n").into_bytes());
        let read = read_token(true, Some("other".into()), &mut input, false).unwrap();
        assert_eq!(read.unwrap().header().1, format!("Bearer {SECRET}"));
    }

    #[test]
    fn an_unset_variable_presents_no_token() {
        let read = read_token(false, None, &mut Cursor::new(Vec::new()), false);
        assert_eq!(read, Ok(None));
    }

    #[test]
    fn an_empty_value_is_rejected_from_either_source() {
        assert_eq!(from_stdin(""), Err(TokenError::Empty(TokenSource::Stdin)));
        assert_eq!(
            from_stdin(" \n"),
            Err(TokenError::Empty(TokenSource::Stdin))
        );
        let env = read_token(false, Some("".into()), &mut Cursor::new(Vec::new()), false);
        assert_eq!(env, Err(TokenError::Empty(TokenSource::Env)));
    }

    #[test]
    fn a_value_a_header_cannot_carry_is_invalid() {
        for bad in ["two words", "tab\there", "caf\u{e9}", "nul\0"] {
            assert_eq!(
                Credential::bearer(bad, TokenSource::Env),
                Err(TokenError::Invalid(TokenSource::Env)),
                "{bad:?}"
            );
        }
        let not_utf8 = read_token(true, None, &mut Cursor::new(vec![0xff, b'\n']), false);
        assert_eq!(not_utf8, Err(TokenError::Invalid(TokenSource::Stdin)));
    }

    #[test]
    fn a_terminal_stdin_is_refused_before_any_read() {
        let mut input = Cursor::new(format!("{SECRET}\n").into_bytes());
        let read = read_token(true, None, &mut input, true);
        assert_eq!(read, Err(TokenError::StdinTerminal));
        assert_eq!(input.position(), 0);
    }

    #[test]
    fn debug_never_prints_the_token() {
        let credential = Credential::bearer(SECRET, TokenSource::Env).unwrap();
        let shown = format!("{credential:?}");
        assert!(!shown.contains(SECRET), "{shown}");
        let headers = vec![
            credential.header(),
            ("Accept".to_string(), "application/json".to_string()),
        ];
        let shown = format!("{:?}", RedactedHeaders(&headers));
        assert!(!shown.contains(SECRET), "{shown}");
        assert!(shown.contains("application/json"), "{shown}");
    }
}
