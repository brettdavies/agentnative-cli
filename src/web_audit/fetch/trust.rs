//! The trust set a TLS handshake verifies against, per locality class.
//!
//! ```text
//! public target ...... bundled Mozilla roots + --ca-cert roots
//! local target ....... bundled Mozilla roots + --ca-cert roots + OS trust store
//! ```
//!
//! A public target keeps the hosted auditor's trust set, so a verdict on a
//! public site matches anc.dev's. A local or private target also trusts the
//! machine's own store, where an internal CA or a development CA such as
//! mkcert's is installed. Chain and hostname verification always run; no
//! setting skips them.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use rustls::RootCertStore;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::{self, PemObject};

use crate::web_audit::locality::Locality;

/// Which trust set a request's handshake verifies against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrustClass {
    /// The bundled roots plus any `--ca-cert` roots.
    #[default]
    Public,
    /// The public set plus the OS trust store.
    Local,
}

impl TrustClass {
    /// The class a host of this locality connects with. A metadata host is
    /// never contacted, so it maps to the narrower set.
    pub fn for_locality(class: Locality) -> Self {
        match class {
            Locality::Local => TrustClass::Local,
            Locality::Public | Locality::Metadata => TrustClass::Public,
        }
    }

    /// The trust set's name in evidence, so a rejected chain says which
    /// roots it was checked against.
    pub fn label(self, with_ca_certs: bool) -> &'static str {
        match (self, with_ca_certs) {
            (TrustClass::Public, false) => "bundled roots",
            (TrustClass::Public, true) => "bundled roots and --ca-cert roots",
            (TrustClass::Local, false) => "bundled roots and the OS trust store",
            (TrustClass::Local, true) => "bundled roots, the OS trust store and --ca-cert roots",
        }
    }
}

/// Where the OS trust store comes from, injectable so a test can prove a
/// public target never reads it.
pub trait OsRoots: Send + Sync {
    /// Every root certificate the store holds. A store that cannot be read
    /// contributes nothing; the bundled roots still apply.
    fn load(&self) -> Vec<CertificateDer<'static>>;
}

/// The machine's trust store, through `rustls-native-certs`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeRoots;

impl OsRoots for NativeRoots {
    fn load(&self) -> Vec<CertificateDer<'static>> {
        rustls_native_certs::load_native_certs().certs
    }
}

/// The roots a run adds beyond the bundled set, and where the OS store is
/// read from when a local target needs it.
#[derive(Clone)]
pub struct TrustConfig {
    ca_certs: Arc<Vec<CertificateDer<'static>>>,
    os_roots: Arc<dyn OsRoots>,
}

impl fmt::Debug for TrustConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TrustConfig")
            .field("ca_certs", &self.ca_certs.len())
            .finish_non_exhaustive()
    }
}

impl Default for TrustConfig {
    fn default() -> Self {
        TrustConfig::new(Vec::new())
    }
}

impl TrustConfig {
    /// The bundled roots plus `ca_certs`, with the machine's OS store for
    /// local targets.
    pub fn new(ca_certs: Vec<CertificateDer<'static>>) -> Self {
        TrustConfig {
            ca_certs: Arc::new(ca_certs),
            os_roots: Arc::new(NativeRoots),
        }
    }

    /// Read the OS store from `os_roots` instead of the machine.
    pub fn with_os_roots(mut self, os_roots: Arc<dyn OsRoots>) -> Self {
        self.os_roots = os_roots;
        self
    }

    /// The trust set's name in evidence for a class.
    pub fn label(&self, class: TrustClass) -> &'static str {
        class.label(!self.ca_certs.is_empty())
    }

    /// The root store a class verifies against. The OS store is read only
    /// for a local class, each time this is called.
    pub fn root_store(&self, class: TrustClass) -> RootCertStore {
        let mut store = RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        store.add_parsable_certificates(self.ca_certs.iter().cloned());
        if class == TrustClass::Local {
            store.add_parsable_certificates(self.os_roots.load());
        }
        store
    }
}

/// Why a `--ca-cert` file supplies no usable root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaCertError {
    /// The file could not be read.
    Unreadable(String),
    /// The file holds no PEM `CERTIFICATE` block.
    NoCertificate,
    /// A PEM block is malformed.
    BadPem(String),
    /// A certificate cannot serve as a trust anchor.
    NotARoot(String),
}

impl fmt::Display for CaCertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaCertError::Unreadable(detail) => write!(f, "cannot read the file: {detail}"),
            CaCertError::NoCertificate => {
                f.write_str("the file holds no PEM certificate (expected a CERTIFICATE block)")
            }
            CaCertError::BadPem(detail) => write!(f, "the file is not valid PEM: {detail}"),
            CaCertError::NotARoot(detail) => {
                write!(
                    f,
                    "a certificate in the file cannot be a trust root: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for CaCertError {}

/// Read every certificate in a PEM file, each one checked as a trust
/// anchor, before any request goes out.
pub fn load_ca_file(path: &Path) -> Result<Vec<CertificateDer<'static>>, CaCertError> {
    let bytes = std::fs::read(path).map_err(|e| CaCertError::Unreadable(e.to_string()))?;
    let certs = CertificateDer::pem_slice_iter(&bytes)
        .collect::<Result<Vec<_>, pem::Error>>()
        .map_err(|e| CaCertError::BadPem(e.to_string()))?;
    if certs.is_empty() {
        return Err(CaCertError::NoCertificate);
    }
    let mut store = RootCertStore::empty();
    for cert in &certs {
        store
            .add(cert.clone())
            .map_err(|e| CaCertError::NotARoot(e.to_string()))?;
    }
    Ok(certs)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    const CA: &[u8] = include_bytes!("../../../tests/fixtures/tls/ca.cert.pem");
    const KEY: &[u8] = include_bytes!("../../../tests/fixtures/tls/localhost.key.pem");

    fn file(name: &str, contents: &[u8]) -> tempfile::TempPath {
        let mut f = tempfile::Builder::new().prefix(name).tempfile().unwrap();
        std::io::Write::write_all(&mut f, contents).unwrap();
        f.into_temp_path()
    }

    #[test]
    fn a_pem_ca_loads_and_each_failure_says_why() {
        assert_eq!(load_ca_file(&file("ca", CA)).unwrap().len(), 1);
        let missing = Path::new("/nonexistent/anc-ca.pem");
        assert!(matches!(
            load_ca_file(missing),
            Err(CaCertError::Unreadable(_))
        ));
        assert_eq!(
            load_ca_file(&file("text", b"not a certificate\n")),
            Err(CaCertError::NoCertificate)
        );
        assert_eq!(
            load_ca_file(&file("key", KEY)),
            Err(CaCertError::NoCertificate)
        );
        let garbage = b"-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";
        assert!(matches!(
            load_ca_file(&file("garbage", garbage)),
            Err(CaCertError::NotARoot(_))
        ));
    }

    #[test]
    fn labels_name_every_root_source_in_the_set() {
        assert_eq!(TrustClass::Public.label(false), "bundled roots");
        assert_eq!(
            TrustClass::Local.label(true),
            "bundled roots, the OS trust store and --ca-cert roots"
        );
        assert_eq!(TrustClass::for_locality(Locality::Local), TrustClass::Local);
        assert_eq!(
            TrustClass::for_locality(Locality::Metadata),
            TrustClass::Public
        );
    }

    struct Counting(AtomicUsize);

    impl OsRoots for Counting {
        fn load(&self) -> Vec<CertificateDer<'static>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Vec::new()
        }
    }

    #[test]
    fn only_a_local_class_reads_the_os_store() {
        let os = Arc::new(Counting(AtomicUsize::new(0)));
        let trust = TrustConfig::new(load_ca_file(&file("ca", CA)).unwrap())
            .with_os_roots(Arc::clone(&os) as Arc<dyn OsRoots>);
        let bundled = webpki_roots::TLS_SERVER_ROOTS.len();
        assert_eq!(trust.root_store(TrustClass::Public).len(), bundled + 1);
        assert_eq!(os.0.load(Ordering::SeqCst), 0);
        trust.root_store(TrustClass::Local);
        assert_eq!(os.0.load(Ordering::SeqCst), 1);
    }
}
