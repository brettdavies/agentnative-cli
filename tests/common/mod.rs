//! In-test servers for the web-audit transport tests: a minimal HTTP/1.1
//! responder that records every request it receives, and a rustls server
//! that presents a fixed certificate chain.

#![allow(dead_code)]

pub mod corpus;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// One request as the server read it off the wire.
#[derive(Clone, Debug)]
pub struct RawRequest {
    pub method: String,
    /// The request target as sent: a path, or an absolute URL when the
    /// client speaks to the server as a proxy.
    pub target: String,
    /// Header names lowercased, in wire order.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl RawRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// What the handler asks the server to write back.
#[derive(Clone, Debug)]
pub struct RawResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl RawResponse {
    pub fn new(status: u16, headers: &[(&str, &str)], body: &[u8]) -> Self {
        RawResponse {
            status,
            headers: headers
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect(),
            body: body.to_vec(),
        }
    }
}

pub struct Server {
    pub addr: SocketAddr,
    hits: Arc<Mutex<Vec<RawRequest>>>,
}

impl Server {
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    pub fn hits(&self) -> Vec<RawRequest> {
        self.hits.lock().unwrap().clone()
    }
}

/// Spawn a plain HTTP server on a loopback port. Each connection carries one
/// request; the response closes it.
pub fn spawn<F>(handler: F) -> Server
where
    F: Fn(&RawRequest) -> RawResponse + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().unwrap();
    let hits: Arc<Mutex<Vec<RawRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&hits);
    let handler = Arc::new(handler);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let recorder = Arc::clone(&recorder);
            let handler = Arc::clone(&handler);
            thread::spawn(move || {
                if let Some(req) = read_request(&stream) {
                    recorder.lock().unwrap().push(req.clone());
                    let resp = handler(&req);
                    write_response(stream, &resp);
                }
            });
        }
    });
    Server { addr, hits }
}

fn read_request(stream: &TcpStream) -> Option<RawRequest> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.trim_end().splitn(3, ' ');
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).ok()?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (name, value) = h.split_once(':')?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    let len: usize = headers
        .iter()
        .find(|(n, _)| n == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    if len > 0 {
        reader.read_exact(&mut body).ok()?;
    }
    Some(RawRequest {
        method,
        target,
        headers,
        body,
    })
}

fn write_response(mut stream: TcpStream, resp: &RawResponse) {
    let reason = match resp.status {
        200 => "OK",
        301 => "Moved Permanently",
        302 => "Found",
        404 => "Not Found",
        _ => "Status",
    };
    let mut out = format!("HTTP/1.1 {} {}\r\n", resp.status, reason);
    for (n, v) in &resp.headers {
        out.push_str(&format!("{n}: {v}\r\n"));
    }
    out.push_str(&format!("content-length: {}\r\n", resp.body.len()));
    out.push_str("connection: close\r\n\r\n");
    let _ = stream.write_all(out.as_bytes());
    let _ = stream.write_all(&resp.body);
    let _ = stream.flush();
}

/// Spawn an HTTP CONNECT proxy on a loopback port. The CONNECT request is
/// recorded with its `host:port` target and answered with 200; the request
/// tunneled after it is recorded with its path and answered by the handler.
/// This is the wire shape ureq's HTTP proxy speaks for every target.
pub fn spawn_connect_proxy<F>(handler: F) -> Server
where
    F: Fn(&RawRequest) -> RawResponse + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().unwrap();
    let hits: Arc<Mutex<Vec<RawRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&hits);
    let handler = Arc::new(handler);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let recorder = Arc::clone(&recorder);
            let handler = Arc::clone(&handler);
            thread::spawn(move || {
                let Some(connect) = read_request(&stream) else {
                    return;
                };
                recorder.lock().unwrap().push(connect.clone());
                if connect.method != "CONNECT" {
                    write_response(stream, &handler(&connect));
                    return;
                }
                let _ = stream.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n");
                let _ = stream.flush();
                if let Some(inner) = read_request(&stream) {
                    recorder.lock().unwrap().push(inner.clone());
                    write_response(stream, &handler(&inner));
                }
            });
        }
    });
    Server { addr, hits }
}

/// Spawn a server that accepts connections and never answers them.
pub fn spawn_silent() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            held.push(stream);
        }
    });
    addr
}

/// Spawn a server that drops its first `closes` connections without
/// writing a byte, then answers every later one with `body`. A server that
/// closes an idle keep-alive connection, or one that answers HTTP/1.0 and
/// closes every connection, looks exactly like this to a client holding a
/// pooled socket. The returned counter carries how many connections were
/// accepted in total.
pub fn spawn_closing(closes: usize, body: &'static str) -> (SocketAddr, Arc<Mutex<usize>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().unwrap();
    let accepted: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let counter = Arc::clone(&accepted);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let seen = {
                let mut n = counter.lock().unwrap();
                *n += 1;
                *n
            };
            if seen <= closes {
                // Close without reading or writing: the client's request
                // never reaches an HTTP responder.
                drop(stream);
                continue;
            }
            if read_request(&stream).is_some() {
                write_response(
                    stream,
                    &RawResponse::new(200, &[("content-type", "text/plain")], body.as_bytes()),
                );
            }
        }
    });
    (addr, accepted)
}

/// Spawn a TLS server presenting the given PEM certificate and key. It
/// completes the handshake when the client accepts the chain and otherwise
/// returns after the client's alert; it never serves HTTP.
pub fn spawn_tls(cert_pem: &[u8], key_pem: &[u8]) -> SocketAddr {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};

    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(cert_pem)
        .collect::<Result<_, _>>()
        .expect("certificate pem");
    let key = PrivateKeyDer::from_pem_slice(key_pem).expect("key pem");
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("server config");
    let config = Arc::new(config);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let config = Arc::clone(&config);
            thread::spawn(move || {
                let mut conn = rustls::ServerConnection::new(config).expect("server connection");
                while conn.is_handshaking() {
                    if conn.complete_io(&mut stream).is_err() {
                        break;
                    }
                }
                conn.send_close_notify();
                let _ = conn.complete_io(&mut stream);
            });
        }
    });
    addr
}
