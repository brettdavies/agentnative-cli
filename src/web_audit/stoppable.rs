//! A transport a run can stop. A blocking read takes only a timeout and ureq
//! keeps its socket to itself, so nothing can reach in and end one. This layer
//! sits directly on the connection and waits for input in short slices, reading
//! a flag between them, so a stopped read ends within one slice however long its
//! timeout has left. A read that has its input returns at once, so a slice costs
//! a request nothing. Connecting cannot be sliced: a connection that finishes
//! after the stop is refused here, before anything is sent on it.

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ureq::unversioned::transport::{Buffers, ConnectionDetails, Connector, NextTimeout, Transport};

const READ_SLICE: Duration = Duration::from_millis(20);

/// Whether a transport's requests have been stopped, for good. Clones share it.
#[derive(Clone, Debug, Default)]
pub struct StopFlag(Arc<AtomicBool>);

impl StopFlag {
    /// End every request in flight and refuse any more.
    pub fn stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether [`StopFlag::stop`] has been called.
    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    fn check(&self) -> Result<(), ureq::Error> {
        if self.is_stopped() {
            return Err(ureq::Error::Io(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "audit ended",
            )));
        }
        Ok(())
    }
}

/// Wraps each connection the chain made, and refuses to pass one on once stopped.
#[derive(Debug)]
pub(crate) struct StoppableConnector(pub(crate) StopFlag);

impl<In: Transport> Connector<In> for StoppableConnector {
    type Out = StoppableTransport<In>;

    fn connect(
        &self,
        _details: &ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        self.0.check()?;
        Ok(chained.map(|inner| StoppableTransport {
            inner,
            stop: self.0.clone(),
        }))
    }
}

#[derive(Debug)]
pub(crate) struct StoppableTransport<T> {
    inner: T,
    stop: StopFlag,
}

impl<T: Transport> Transport for StoppableTransport<T> {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.stop.check()?;
        self.inner.transmit_output(amount, timeout)
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        let started = Instant::now();
        loop {
            self.stop.check()?;
            let left = timeout.after.saturating_sub(started.elapsed());
            let slice = NextTimeout {
                after: left.min(READ_SLICE).into(),
                ..timeout
            };
            match self.inner.await_input(slice) {
                Err(ureq::Error::Timeout(_)) if left > READ_SLICE => {}
                answer => return answer,
            }
        }
    }

    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}
