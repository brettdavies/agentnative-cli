//! Which connections the production transport keeps. A kept connection saves
//! the next request to its host a TLS handshake, and an audit sends dozens of
//! requests to one host, so a connection lost here is wall clock the user waits.

#![cfg(feature = "web")]

mod common;

use std::io::Read;
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use agentnative::web_audit::fetch::trust::{TrustClass, TrustConfig, load_ca_file};
use agentnative::web_audit::transport::{
    IDLE_CONNECTIONS, IDLE_CONNECTIONS_PER_HOST, Request, Transport, UreqTransport,
};
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
fn a_gzip_body_with_trailing_bytes_decodes_and_leaves_its_connection() {
    let mut body = common::gzip_of(DECODED);
    body.push(b'\n');
    let server = common::spawn_https_keep_alive(move |_| {
        RawResponse::new(200, &[("content-encoding", "gzip")], &body)
    });
    let transport = transport();
    let url = server.server.url("/");
    for _ in 0..2 {
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

const BURST: usize = IDLE_CONNECTIONS_PER_HOST;

/// A server that holds every request until `at_once` have arrived, so each of
/// them is on a connection of its own.
fn held_server(at_once: usize) -> common::KeepAliveServer {
    let arrived = Arc::new(Barrier::new(at_once));
    common::spawn_https_keep_alive(move |_| {
        arrived.wait();
        RawResponse::new(200, &[], b"ok")
    })
}

fn at_once(transport: &UreqTransport, url: &str, count: usize) {
    thread::scope(|scope| {
        for _ in 0..count {
            scope.spawn(|| assert_eq!(read_body(transport, url), b"ok"));
        }
    });
}

#[test]
fn a_burst_leaves_a_connection_for_each_request_of_the_next() {
    let server = held_server(BURST);
    let transport = transport();
    let url = server.server.url("/");
    at_once(&transport, &url, BURST);
    assert_eq!(server.accepted(), BURST);
    at_once(&transport, &url, BURST);
    assert_eq!(
        server.accepted(),
        BURST,
        "the second burst reuses every connection of the first"
    );
}

#[test]
fn the_hosts_of_one_audit_keep_their_connections_side_by_side() {
    let hosts = IDLE_CONNECTIONS / IDLE_CONNECTIONS_PER_HOST;
    let servers: Vec<_> = (0..hosts).map(|_| held_server(BURST)).collect();
    let transport = transport();
    for _ in 0..2 {
        for server in &servers {
            at_once(&transport, &server.server.url("/"), BURST);
        }
    }
    for server in &servers {
        assert_eq!(server.accepted(), BURST);
    }
}

#[test]
fn a_request_outlasts_more_dead_connections_than_it_has_attempts() {
    let arrived = Arc::new(Barrier::new(BURST));
    let seen = Arc::new(AtomicUsize::new(0));
    let server = common::spawn_https_dying_on_reuse(
        move |_| {
            if seen.fetch_add(1, Ordering::SeqCst) < BURST {
                arrived.wait();
            }
            RawResponse::new(200, &[], b"ok")
        },
        |_| true,
    );
    let transport = transport();
    let url = server.server.url("/");
    at_once(&transport, &url, BURST);
    assert_eq!(read_body(&transport, &url), b"ok");
    assert_eq!(server.accepted(), BURST + 1);
}

#[test]
fn a_retry_leaves_the_hosts_idle_connections_alone() {
    let arrived = Arc::new(Barrier::new(BURST));
    let server = common::spawn_https_dying_on_reuse(
        move |request| {
            if request.target == "/" {
                arrived.wait();
            }
            RawResponse::new(200, &[], b"ok")
        },
        |request| request.target == "/dies",
    );
    let transport = transport();
    let url = server.server.url("/");
    at_once(&transport, &url, BURST);
    assert_eq!(read_body(&transport, &server.server.url("/dies")), b"ok");
    assert_eq!(
        server.accepted(),
        BURST + 1,
        "one retry, on a new connection"
    );
    at_once(&transport, &url, BURST);
    assert_eq!(
        server.accepted(),
        BURST + 2,
        "the next burst has every idle connection but the one that died"
    );
}

#[test]
fn a_post_goes_again_only_once() {
    let (addr, accepted) = common::spawn_closing(usize::MAX, "");
    let post = Request {
        method: "POST".to_string(),
        body: Some(b"{}".to_vec()),
        ..get(&format!("http://{addr}/mcp"))
    };
    assert!(transport().send(&post).is_err());
    assert_eq!(*accepted.lock().unwrap(), 2);
}

#[test]
fn a_retry_spends_what_is_left_of_the_requests_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().unwrap();
    let (retried, heard) = mpsc::channel();
    thread::spawn(move || {
        let (first, _) = listener.accept().expect("the first attempt");
        thread::sleep(Duration::from_millis(1500));
        drop(first);
        // The second attempt connects and hears nothing.
        let held = listener.accept();
        let _ = retried.send(());
        drop(held);
        thread::sleep(Duration::from_secs(10));
    });
    let request = Request {
        timeout: Duration::from_secs(4),
        ..get(&format!("http://{addr}/"))
    };
    let started = Instant::now();
    assert!(transport().send(&request).is_err());
    let took = started.elapsed();
    assert!(heard.try_recv().is_ok(), "the request went again");
    assert!(took < Duration::from_millis(4800), "{took:?}");
}
