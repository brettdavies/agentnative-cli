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

    pub(crate) fn is_stopped(&self) -> bool {
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
            // ureq reads a zero timeout as a whole second, which would outlast the
            // budget a slice that overran has just used up.
            if left.is_zero() {
                return Err(ureq::Error::Timeout(timeout.reason));
            }
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use ureq::unversioned::transport::LazyBuffers;

    use super::*;

    /// Every wait overruns the slice it was given, as on a loaded machine.
    #[derive(Debug)]
    struct Overrunning {
        buffers: LazyBuffers,
        slices: Arc<Mutex<Vec<Duration>>>,
    }

    impl Transport for Overrunning {
        fn buffers(&mut self) -> &mut dyn Buffers {
            &mut self.buffers
        }

        fn transmit_output(
            &mut self,
            _amount: usize,
            _timeout: NextTimeout,
        ) -> Result<(), ureq::Error> {
            Ok(())
        }

        fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
            self.slices.lock().unwrap().push(*timeout.after);
            std::thread::sleep(Duration::from_millis(60));
            Err(ureq::Error::Timeout(timeout.reason))
        }

        fn is_open(&mut self) -> bool {
            true
        }
    }

    #[test]
    fn a_slice_that_overruns_the_budget_ends_the_wait_rather_than_asking_for_a_zero_read() {
        let slices = Arc::new(Mutex::new(Vec::new()));
        let mut transport = StoppableTransport {
            inner: Overrunning {
                buffers: LazyBuffers::new(64, 64),
                slices: slices.clone(),
            },
            stop: StopFlag::default(),
        };
        let answer = transport.await_input(NextTimeout {
            after: Duration::from_millis(40).into(),
            reason: ureq::Timeout::Global,
            per_read: None,
            per_write: None,
        });
        assert!(matches!(answer, Err(ureq::Error::Timeout(_))), "{answer:?}");
        let slices = slices.lock().unwrap();
        assert!(slices.iter().all(|slice| !slice.is_zero()), "{slices:?}");
        assert_eq!(slices.len(), 1, "{slices:?}");
    }
}
