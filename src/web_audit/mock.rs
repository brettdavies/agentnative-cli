//! A scripted [`Transport`] for tests and the conformance corpus.
//!
//! Rules match requests the way the site's stub `fetchImpl` does, so a
//! corpus scenario drives both engines through one contract: method
//! (case-insensitive), URL (equal after parsing), a subset of headers
//! (lowercased names, exact values), the JSON-RPC `method` of the body, and a
//! body substring. The first matching rule answers; a request no rule matches
//! gets the unmatched policy. Every request is recorded, so a test can assert
//! which hosts a run touched.

use std::io::Cursor;
use std::sync::Mutex;

use super::headers::Headers;
use super::transport::{Request, Response, Transport, TransportError};

/// What a rule answers with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MockResponse {
    /// A full response.
    Ok {
        /// HTTP status code.
        status: u16,
        /// Response headers as written; names are lowercased on the way out.
        headers: Vec<(String, String)>,
        /// Response body bytes.
        body: Vec<u8>,
    },
    /// A transport failure surfaced with exactly this message.
    Error(String),
}

impl MockResponse {
    /// A full response.
    pub fn ok(status: u16, headers: &[(&str, &str)], body: &[u8]) -> Self {
        MockResponse::Ok {
            status,
            headers: headers
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect(),
            body: body.to_vec(),
        }
    }

    /// A transport failure carrying this message verbatim.
    pub fn error(message: &str) -> Self {
        MockResponse::Error(message.to_string())
    }
}

/// One matching rule and its answer.
#[derive(Clone, Debug)]
pub struct MockRule {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body_json_method: Option<String>,
    body_contains: Option<String>,
    response: MockResponse,
}

impl MockRule {
    /// A rule matching this method and URL, answering 200 with an empty body
    /// until [`MockRule::response`] replaces that.
    pub fn new(method: &str, url: &str) -> Self {
        MockRule {
            method: method.to_ascii_uppercase(),
            url: normalize_url(url),
            headers: Vec::new(),
            body_json_method: None,
            body_contains: None,
            response: MockResponse::ok(200, &[], b""),
        }
    }

    /// Require this request header, matched by lowercased name and exact value.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers
            .push((name.to_ascii_lowercase(), value.to_string()));
        self
    }

    /// Require the request body to be a JSON object whose `method` is this.
    pub fn body_json_method(mut self, method: &str) -> Self {
        self.body_json_method = Some(method.to_string());
        self
    }

    /// Require the raw request body to contain this substring.
    pub fn body_contains(mut self, needle: &str) -> Self {
        self.body_contains = Some(needle.to_string());
        self
    }

    /// The answer for a matching request.
    pub fn response(mut self, response: MockResponse) -> Self {
        self.response = response;
        self
    }

    fn matches(&self, request: &Request) -> bool {
        if !request.method.eq_ignore_ascii_case(&self.method) {
            return false;
        }
        if normalize_url(&request.url) != self.url {
            return false;
        }
        let has_header = |(name, value): &(String, String)| {
            request
                .headers
                .iter()
                .any(|(n, v)| n.eq_ignore_ascii_case(name) && v == value)
        };
        if !self.headers.iter().all(has_header) {
            return false;
        }
        let body = request.body.as_deref().unwrap_or(&[]);
        if let Some(wanted) = &self.body_json_method {
            let method = serde_json::from_slice::<serde_json::Value>(body)
                .ok()
                .and_then(|v| v.get("method").and_then(|m| m.as_str().map(String::from)));
            if method.as_deref() != Some(wanted.as_str()) {
                return false;
            }
        }
        if let Some(needle) = &self.body_contains
            && !String::from_utf8_lossy(body).contains(needle.as_str())
        {
            return false;
        }
        true
    }
}

fn normalize_url(raw: &str) -> String {
    url::Url::parse(raw)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| raw.to_string())
}

/// A transport answering from rules and recording every request.
#[derive(Debug)]
pub struct MockTransport {
    rules: Vec<MockRule>,
    unmatched: MockResponse,
    seen: Mutex<Vec<Request>>,
}

impl MockTransport {
    /// Rules in priority order, plus the answer for anything they miss.
    pub fn new(rules: Vec<MockRule>, unmatched: MockResponse) -> Self {
        MockTransport {
            rules,
            unmatched,
            seen: Mutex::new(Vec::new()),
        }
    }

    /// Every request received so far, in order.
    pub fn requests(&self) -> Vec<Request> {
        self.seen.lock().expect("mock transport lock").clone()
    }
}

impl Transport for MockTransport {
    fn send(&self, request: &Request) -> Result<Response, TransportError> {
        self.seen
            .lock()
            .expect("mock transport lock")
            .push(request.clone());
        let answer = self
            .rules
            .iter()
            .find(|rule| rule.matches(request))
            .map_or(&self.unmatched, |rule| &rule.response);
        match answer {
            MockResponse::Ok {
                status,
                headers,
                body,
            } => Ok(Response {
                status: *status,
                headers: Headers::from_pairs(headers.iter().map(|(n, v)| (n, v))),
                body: Box::new(Cursor::new(body.clone())),
            }),
            MockResponse::Error(message) => Err(TransportError::Verbatim(message.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn request(method: &str, url: &str) -> Request {
        Request {
            method: method.to_string(),
            url: url.to_string(),
            headers: Vec::new(),
            body: None,
            timeout: Duration::from_secs(1),
            via_proxy: false,
        }
    }

    #[test]
    fn url_matching_normalizes_both_sides() {
        let rule = MockRule::new("get", "HTTPS://Example.com");
        assert!(rule.matches(&request("GET", "https://example.com/")));
        assert!(!rule.matches(&request("GET", "https://example.com/other")));
        assert!(!rule.matches(&request("POST", "https://example.com/")));
    }

    #[test]
    fn body_predicates_gate_the_match() {
        let rule = MockRule::new("POST", "https://example.com/mcp")
            .body_json_method("initialize")
            .body_contains("2025-06-18");
        let mut req = request("POST", "https://example.com/mcp");
        req.body =
            Some(br#"{"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#.to_vec());
        assert!(rule.matches(&req));
        req.body = Some(br#"{"method":"initialize"}"#.to_vec());
        assert!(!rule.matches(&req));
        req.body = Some(b"not json 2025-06-18".to_vec());
        assert!(!rule.matches(&req));
    }

    #[test]
    fn the_first_matching_rule_wins_and_requests_are_recorded() {
        let transport = MockTransport::new(
            vec![
                MockRule::new("GET", "https://example.com/").response(MockResponse::ok(
                    200,
                    &[],
                    b"first",
                )),
                MockRule::new("GET", "https://example.com/").response(MockResponse::ok(
                    500,
                    &[],
                    b"second",
                )),
            ],
            MockResponse::error("connection refused"),
        );
        let ok = transport
            .send(&request("GET", "https://example.com/"))
            .expect("matched");
        assert_eq!(ok.status, 200);
        let err = transport
            .send(&request("GET", "https://example.com/missing"))
            .expect_err("unmatched");
        assert_eq!(err.to_string(), "connection refused");
        assert_eq!(transport.requests().len(), 2);
    }
}
