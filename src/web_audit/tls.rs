//! TLS for the transport's agents: one rustls client config per trust
//! class, wrapped around ureq's own TCP and CONNECT-proxy connectors.
//!
//! ureq's built-in rustls connector derives its root store from a single
//! `RootCerts` value, which holds either the bundled trust anchors or a list
//! of certificates, never both. A trust class needs the bundled anchors plus
//! `--ca-cert` roots, and for a local target the OS store as well, so this
//! connector takes a finished `ClientConfig` and performs the handshake the
//! way ureq's does: SNI from the URI host, the connect timeout over the
//! handshake, and the same `rustls::Error` inside ureq's error that the
//! transport classifies.

use std::fmt;
use std::io::{Read, Write};
use std::sync::Arc;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use ureq::unversioned::resolver::DefaultResolver;
use ureq::unversioned::transport::{
    Buffers, ConnectProxyConnector, ConnectionDetails, Connector, Either, LazyBuffers, NextTimeout,
    TcpConnector, Transport, TransportAdapter,
};

use super::fetch::trust::{TrustClass, TrustConfig};

/// The rustls client config for one trust class: TLS 1.2 and 1.3 under the
/// aws-lc-rs provider, no client certificate.
pub fn client_config(trust: &TrustConfig, class: TrustClass) -> Arc<ClientConfig> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(rustls::ALL_VERSIONS)
        .expect("aws-lc-rs supports every rustls protocol version")
        .with_root_certificates(trust.root_store(class))
        .with_no_client_auth();
    Arc::new(config)
}

/// An agent whose HTTPS connections verify against `tls`.
pub fn agent(config: ureq::config::Config, tls: Arc<ClientConfig>) -> ureq::Agent {
    let connector = ConnectProxyConnector::default()
        .chain(TcpConnector::default())
        .chain(TlsConnector { config: tls });
    ureq::Agent::with_parts(config, connector, DefaultResolver::default())
}

/// Wraps the chained connection in TLS when the URI is `https`.
struct TlsConnector {
    config: Arc<ClientConfig>,
}

impl fmt::Debug for TlsConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsConnector").finish_non_exhaustive()
    }
}

impl<In: Transport> Connector<In> for TlsConnector {
    type Out = Either<In, TlsTransport>;

    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        let Some(transport) = chained else {
            return Ok(None);
        };
        if !details.needs_tls() || transport.is_tls() {
            return Ok(Some(Either::A(transport)));
        }
        let host = details.uri.host().ok_or(ureq::Error::Tls(
            "no host to verify the certificate against",
        ))?;
        let bare = host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .unwrap_or(host);
        let name = ServerName::try_from(bare)
            .map_err(|_| ureq::Error::Tls("invalid server name for TLS"))?
            .to_owned();
        let mut conn = ClientConnection::new(Arc::clone(&self.config), name)?;
        let mut sock = TransportAdapter::new(transport.boxed());
        sock.set_timeout(details.timeout);
        conn.complete_io(&mut sock)?;
        let buffers = LazyBuffers::new(
            details.config.input_buffer_size(),
            details.config.output_buffer_size(),
        );
        Ok(Some(Either::B(TlsTransport {
            buffers,
            stream: StreamOwned { conn, sock },
        })))
    }
}

/// An established TLS connection, as ureq's HTTP layer drives it.
struct TlsTransport {
    buffers: LazyBuffers,
    stream: StreamOwned<ClientConnection, TransportAdapter>,
}

impl fmt::Debug for TlsTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsTransport").finish_non_exhaustive()
    }
}

impl Transport for TlsTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.stream.get_mut().set_timeout(timeout);
        let output = &self.buffers.output()[..amount];
        self.stream.write_all(output)?;
        Ok(())
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        self.stream.get_mut().set_timeout(timeout);
        let input = self.buffers.input_append_buf();
        let amount = self.stream.read(input)?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        self.stream.get_mut().get_mut().is_open()
    }

    fn is_tls(&self) -> bool {
        true
    }
}
