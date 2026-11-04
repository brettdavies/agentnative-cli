// Full TLS handshakes with resumption off, timed from ClientHello to Finished. The TCP connect is
// outside the timer. Usage: hs <host> <port> <rounds> [ca.pem]
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Instant;

use rustls::client::Resumption;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (host, port, rounds) = (&args[1], args[2].parse::<u16>().unwrap(), args[3].parse::<usize>().unwrap());
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if let Some(ca) = args.get(4) {
        for cert in CertificateDer::pem_file_iter(ca).unwrap() {
            roots.add(cert.unwrap()).unwrap();
        }
    }
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(rustls::ALL_VERSIONS)
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.resumption = Resumption::disabled();
    let config = Arc::new(config);
    let name = ServerName::try_from(host.clone()).unwrap();
    let mut samples = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        let mut sock = TcpStream::connect((host.as_str(), port)).unwrap();
        sock.set_nodelay(true).unwrap();
        let mut conn = ClientConnection::new(Arc::clone(&config), name.clone()).unwrap();
        let t = Instant::now();
        while conn.is_handshaking() {
            conn.complete_io(&mut sock).unwrap();
        }
        samples.push(t.elapsed().as_micros());
    }
    samples.sort_unstable();
    println!("{}\t{}", samples[rounds / 2], samples[rounds * 9 / 10]);
}
