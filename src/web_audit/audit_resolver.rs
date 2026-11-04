//! One audit's name lookups, shared by classification and the connection.
//!
//! The locality rules classify a host by the addresses its name resolves to,
//! and the connection then needs an address for that same name. Two lookups
//! leave a window in which the name can move, by a short TTL or on purpose: a
//! name that answers public for the classifier and private for the connection
//! reaches an address the rules never saw. Here a name resolves once per
//! audit, so the addresses the rules classified are the addresses the
//! connection uses. A connection through a proxy stays outside this: the proxy
//! resolves the target itself.
//!
//! ```text
//! classify_host -- Resolver ---------.
//!                                     >--> AuditResolver --> one lookup per name
//! ureq agent ----- ConnectResolver --'
//! ```
//!
//! The first caller's lookup runs on its own thread, so a caller with a
//! deadline can stop waiting for it, and every caller reads the same answer.
//! A lookup that finds nothing is not kept: a later caller asks again.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use ureq::config::Config;
use ureq::http::Uri;
use ureq::unversioned::resolver::{self, DefaultResolver, ResolvedSocketAddrs};
use ureq::unversioned::transport::NextTimeout;

use super::locality::{Resolver, SystemResolver};

type Answer = Result<Arc<[IpAddr]>, String>;
type Names = Mutex<HashMap<String, Arc<Pending>>>;

/// The addresses one audit's names resolve to, each looked up once.
pub struct AuditResolver {
    lookup: Arc<dyn Resolver + Send + Sync>,
    names: Arc<Names>,
}

impl std::fmt::Debug for AuditResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditResolver")
            .field("names", &lock(&self.names).len())
            .finish_non_exhaustive()
    }
}

impl AuditResolver {
    /// One audit's lookups through `lookup`.
    pub fn new(lookup: Arc<dyn Resolver + Send + Sync>) -> Self {
        AuditResolver {
            lookup,
            names: Arc::default(),
        }
    }

    /// One audit's lookups through the machine's own resolver.
    pub fn system() -> Self {
        Self::new(Arc::new(SystemResolver))
    }

    /// The answer for `host`, or `None` when `within` passed first.
    fn answer(&self, host: &str, within: Option<Duration>) -> Option<Answer> {
        let name = name_of(host);
        let pending = {
            let mut names = lock(&self.names);
            match names.get(&name) {
                Some(pending) => Arc::clone(pending),
                None => {
                    let pending = Arc::new(Pending::default());
                    names.insert(name.clone(), Arc::clone(&pending));
                    self.spawn_lookup(name, Arc::clone(&pending));
                    pending
                }
            }
        };
        pending.wait(within)
    }

    fn spawn_lookup(&self, name: String, pending: Arc<Pending>) {
        let (lookup, names) = (Arc::clone(&self.lookup), Arc::clone(&self.names));
        thread::spawn(move || {
            // A lookup that panics still answers, so no caller waits on it forever.
            let answer = catch_unwind(AssertUnwindSafe(|| lookup.resolve(&name)))
                .unwrap_or_else(|_| Err("the name lookup panicked".to_string()))
                .map(Arc::<[IpAddr]>::from);
            if answer.as_ref().map_or(true, |addrs| addrs.is_empty()) {
                lock(&names).remove(&name);
            }
            *lock(&pending.answer) = Some(answer);
            pending.settled.notify_all();
        });
    }
}

impl Resolver for AuditResolver {
    fn resolve(&self, host: &str) -> Result<Vec<IpAddr>, String> {
        self.answer(host, None)
            .unwrap_or_else(|| Err("lookup ended without an answer".to_string()))
            .map(|addrs| addrs.to_vec())
    }
}

/// How a transport's agents reach the audit's lookups.
#[derive(Debug)]
pub struct ConnectResolver(pub Arc<AuditResolver>);

impl resolver::Resolver for ConnectResolver {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let host_port = uri
            .scheme()
            .zip(uri.authority())
            .and_then(|(scheme, authority)| DefaultResolver::host_and_port(scheme, authority));
        let Some((host, port)) = host_port.as_deref().and_then(split_host_port) else {
            // ureq's own account of a URI it cannot connect to.
            return DefaultResolver::default().resolve(uri, config, timeout);
        };
        let mut addrs = self.empty();
        if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
            addrs.push(SocketAddr::new(ip, port));
            return Ok(addrs);
        }
        match self.0.answer(host, Some(*timeout.after)) {
            None => Err(ureq::Error::Timeout(timeout.reason)),
            Some(Err(detail)) => Err(ureq::Error::Io(io::Error::other(detail))),
            Some(Ok(ips)) if ips.is_empty() => Err(ureq::Error::HostNotFound),
            Some(Ok(ips)) => {
                for ip in ips.iter() {
                    if addrs.try_push(SocketAddr::new(*ip, port)).is_err() {
                        break;
                    }
                }
                Ok(addrs)
            }
        }
    }
}

fn split_host_port(host_port: &str) -> Option<(&str, u16)> {
    let (host, port) = host_port.rsplit_once(':')?;
    Some((host, port.parse().ok()?))
}

// DNS names are case-insensitive, and the fetch layer's URL and ureq's URI can
// spell one name with and without its root dot: both must reach one answer, or
// the connection looks the name up again.
fn name_of(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

#[derive(Default)]
struct Pending {
    answer: Mutex<Option<Answer>>,
    settled: Condvar,
}

impl Pending {
    fn wait(&self, within: Option<Duration>) -> Option<Answer> {
        let unsettled = |answer: &mut Option<Answer>| answer.is_none();
        let answer = lock(&self.answer);
        let answer = match within {
            None => self
                .settled
                .wait_while(answer, unsettled)
                .unwrap_or_else(PoisonError::into_inner),
            Some(limit) => {
                self.settled
                    .wait_timeout_while(answer, limit, unsettled)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0
            }
        };
        answer.clone()
    }
}

// Every write under these locks is one whole assignment, so a panic elsewhere
// poisons nothing a reader cannot still use.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Barrier, mpsc};

    use ureq::Timeout;
    use ureq::unversioned::resolver::Resolver as _;
    use ureq::unversioned::transport::time;

    use super::*;

    /// Answers by how many lookups came before, counting them.
    struct Counted {
        lookups: AtomicUsize,
        answer: fn(usize) -> Result<Vec<IpAddr>, String>,
    }

    impl Counted {
        fn new(answer: fn(usize) -> Result<Vec<IpAddr>, String>) -> Arc<Self> {
            Arc::new(Counted {
                lookups: AtomicUsize::new(0),
                answer,
            })
        }
    }

    impl Resolver for Counted {
        fn resolve(&self, _: &str) -> Result<Vec<IpAddr>, String> {
            (self.answer)(self.lookups.fetch_add(1, Ordering::SeqCst))
        }
    }

    fn public(_: usize) -> Result<Vec<IpAddr>, String> {
        Ok(vec!["203.0.113.7".parse().unwrap()])
    }

    fn connect_answer(
        lookup: Arc<dyn Resolver + Send + Sync>,
        uri: &str,
    ) -> Result<Vec<SocketAddr>, ureq::Error> {
        let timeout = NextTimeout {
            after: time::Duration::Exact(Duration::from_secs(5)),
            reason: Timeout::Global,
            ..NextTimeout::default()
        };
        ConnectResolver(Arc::new(AuditResolver::new(lookup)))
            .resolve(&uri.parse().unwrap(), &Config::default(), timeout)
            .map(|addrs| addrs.iter().copied().collect())
    }

    #[test]
    fn callers_at_once_share_one_lookup() {
        let lookup = Counted::new(public);
        let resolver = AuditResolver::new(lookup.clone());
        let start = Barrier::new(8);
        thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    start.wait();
                    assert!(resolver.resolve("shared.example").is_ok());
                });
            }
        });
        assert_eq!(lookup.lookups.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn one_name_spelled_two_ways_is_one_lookup() {
        let lookup = Counted::new(public);
        let resolver = AuditResolver::new(lookup.clone());
        for spelling in ["example.com", "EXAMPLE.com", "example.com."] {
            assert!(resolver.resolve(spelling).is_ok(), "{spelling}");
        }
        assert_eq!(lookup.lookups.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_failed_lookup_is_not_kept() {
        let lookup = Counted::new(|seen| match seen {
            0 => Err("temporary failure".to_string()),
            _ => public(seen),
        });
        let resolver = AuditResolver::new(lookup.clone());
        assert_eq!(
            resolver.resolve("flaky.example"),
            Err("temporary failure".to_string())
        );
        assert_eq!(resolver.resolve("flaky.example"), public(1));
        assert_eq!(resolver.resolve("flaky.example"), public(2));
        assert_eq!(lookup.lookups.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_lookup_with_no_addresses_is_not_kept() {
        let lookup = Counted::new(|seen| match seen {
            0 => Ok(Vec::new()),
            _ => public(seen),
        });
        let resolver = AuditResolver::new(lookup.clone());
        assert_eq!(resolver.resolve("empty.example"), Ok(Vec::new()));
        assert_eq!(resolver.resolve("empty.example"), public(1));
    }

    #[test]
    fn a_lookup_that_panics_answers_its_callers_with_a_failure() {
        let lookup = Counted::new(|_| panic!("resolver bug"));
        let resolver = AuditResolver::new(lookup);
        let answer = resolver.answer("panics.example", Some(Duration::from_secs(5)));
        assert_eq!(
            answer.expect("the wait ends with an answer"),
            Err("the name lookup panicked".to_string())
        );
    }

    #[test]
    fn a_connection_gets_every_address_in_the_order_the_resolver_gave_them() {
        let lookup = Counted::new(|_| {
            Ok(vec![
                "2001:db8::1".parse().unwrap(),
                "203.0.113.7".parse().unwrap(),
            ])
        });
        let addrs = connect_answer(lookup, "https://ordered.example:8443/").unwrap();
        assert_eq!(
            addrs,
            vec![
                "[2001:db8::1]:8443".parse::<SocketAddr>().unwrap(),
                "203.0.113.7:8443".parse().unwrap(),
            ]
        );
    }

    #[test]
    fn a_name_with_more_addresses_than_a_connection_holds_keeps_the_first_ones() {
        let lookup =
            Counted::new(|_| Ok((1..=40u8).map(|n| IpAddr::from([203, 0, 113, n])).collect()));
        let addrs = connect_answer(lookup, "https://many.example/").unwrap();
        assert_eq!(addrs.len(), 16);
        assert_eq!(addrs[0], "203.0.113.1:443".parse::<SocketAddr>().unwrap());
        assert_eq!(addrs[15], "203.0.113.16:443".parse::<SocketAddr>().unwrap());
    }

    #[test]
    fn an_address_literal_is_no_lookup() {
        let lookup = Counted::new(public);
        for (uri, addr) in [
            ("http://127.0.0.1:8787/", "127.0.0.1:8787"),
            ("https://[::1]/", "[::1]:443"),
        ] {
            let addrs = connect_answer(lookup.clone(), uri).unwrap();
            assert_eq!(addrs, vec![addr.parse::<SocketAddr>().unwrap()], "{uri}");
        }
        assert_eq!(lookup.lookups.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_lookup_that_answers_after_a_caller_gave_up_serves_the_next_caller() {
        struct Held(Mutex<mpsc::Receiver<()>>);
        impl Resolver for Held {
            fn resolve(&self, _: &str) -> Result<Vec<IpAddr>, String> {
                let _ = lock(&self.0).recv_timeout(Duration::from_secs(5));
                public(0)
            }
        }
        let (release, held) = mpsc::channel();
        let resolver = AuditResolver::new(Arc::new(Held(Mutex::new(held))));
        let waited = resolver.answer("slow.example", Some(Duration::from_millis(20)));
        assert!(waited.is_none(), "{waited:?}");
        release.send(()).unwrap();
        assert_eq!(resolver.resolve("slow.example"), public(0));
    }
}
