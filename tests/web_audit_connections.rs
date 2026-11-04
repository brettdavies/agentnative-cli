//! Which connections the production transport keeps. A kept connection saves
//! the next request to its host a TLS handshake, and an audit sends dozens of
//! requests to one host, so a connection lost here is wall clock the user waits.

#![cfg(feature = "web")]

mod common;

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use agentnative::web_audit::fetch::trust::{TrustClass, TrustConfig, load_ca_file};
use agentnative::web_audit::transport::{Request, Transport, UreqTransport};
use common::{RawRequest, RawResponse};

const UA: &str = "anc-web-audit-test/0 (+https://anc.dev/audit)";

fn transport() -> UreqTransport {
    let ca = load_ca_file(Path::new(common::test_ca_path())).expect("the test CA loads");
    UreqTransport::with_trust(UA, None, TrustConfig::new(ca))
}

fn get(url: &str) -> Request {
    Request {
        method: "GET".to_string(),
        url: url.to_string(),
        headers: Vec::new(),
        body: None,
        timeout: Duration::from_secs(10),
        via_proxy: false,
        trust: TrustClass::Public,
    }
}

/// The body every send reads to its end, which is what returns a connection
/// to the pool.
fn read_body(transport: &UreqTransport, url: &str) -> Vec<u8> {
    let mut response = transport.send(&get(url)).expect("response");
    let mut body = Vec::new();
    response.body.read_to_end(&mut body).expect("body");
    body
}

const DECODED: &[u8] = b"hello gzip body\n";

/// `hello gzip body\n`, through a brotli encoder.
const BROTLI_BODY: &[u8] = &[
    0x8b, 0x07, 0x80, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x67, 0x7a, 0x69, 0x70, 0x20, 0x62, 0x6f,
    0x64, 0x79, 0x0a, 0x03,
];

/// What a CDN does: the best encoding the request advertised.
fn negotiated(request: &RawRequest) -> RawResponse {
    let accepted = request.header("accept-encoding").unwrap_or("");
    if accepted.split(',').any(|e| e.trim() == "br") {
        RawResponse::new(200, &[("content-encoding", "br")], BROTLI_BODY)
    } else {
        RawResponse::new(
            200,
            &[("content-encoding", "gzip")],
            &common::gzip_of(DECODED),
        )
    }
}

#[test]
fn a_negotiated_body_decodes_and_leaves_its_connection_for_the_next_request() {
    let server = common::spawn_https_keep_alive(negotiated);
    let transport = transport();
    let url = server.server.url("/");
    for _ in 0..4 {
        assert_eq!(read_body(&transport, &url), DECODED);
    }
    assert_eq!(server.server.hits().len(), 4);
    assert_eq!(
        server.accepted(),
        1,
        "every request after the first reuses the first connection"
    );
}

#[test]
fn a_chunked_gzip_body_decodes_and_leaves_its_connection_for_the_next_request() {
    let server = common::spawn_https_keep_alive(|_| {
        RawResponse::new(
            200,
            &[("content-encoding", "gzip")],
            &common::gzip_of(DECODED),
        )
        .chunked()
    });
    let transport = transport();
    let url = server.server.url("/");
    for _ in 0..4 {
        assert_eq!(read_body(&transport, &url), DECODED);
    }
    assert_eq!(server.accepted(), 1);
}

#[test]
fn a_brotli_body_sent_unasked_reaches_the_reader_as_it_was_sent() {
    let server = common::spawn_https_keep_alive(|_| {
        RawResponse::new(200, &[("content-encoding", "br")], BROTLI_BODY)
    });
    let transport = transport();
    assert_eq!(read_body(&transport, &server.server.url("/")), BROTLI_BODY);
}

#[test]
fn a_request_advertises_the_encodings_whose_connection_comes_back() {
    let server = common::spawn_https_keep_alive(negotiated);
    let transport = transport();
    assert_eq!(read_body(&transport, &server.server.url("/")), DECODED);
    let hits = server.server.hits();
    assert_eq!(hits[0].header("accept-encoding"), Some("gzip"));
}
