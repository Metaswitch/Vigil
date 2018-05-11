//! A crate to watch over code and ensure that it is still making progress.  The idea is that the
//! code under the vigil should indicate it is alive at regular intervals.  If the code doesn't
//! keep up with its notifications, registered callbacks will be run which may make an attempt to
//! produce diagnostics/kill stalled work/abort the process.
//!
//! If the code under test knows it will not be reporting liveness for a longer than usual period,
//! it can pre-declare this to the vigil by extending the check interval (the code should be
//! careful to reset the interval once the long-standing operation is complete).
#[macro_use]
extern crate log;

use std::thread;
use std::sync::atomic;
use std::sync::Arc;
use std::time::Duration;

const INIT: usize = 0;
const LIVE: usize = 1;
const TEST: usize = 2;
const RISK: usize = 3;
const DEAD: usize = 4;

/// Represents a single vigil over the code.  Should be notified every `tick_interval`, if enough
/// intervals pass without a notification the callback will be fired (on a separate thread).
pub struct Vigil {
    shared: Arc<VigilShared>,
}

impl Vigil {
    /// Create a new vigil object.  The three callbacks are all optional.  Note that no callbacks
    /// will be fired until the first notification has occurred (this allows the vigil to be
    /// created ahead of the worker thread without causing spurious logs/callbacks).
    pub fn create(
        interval_ms: usize,
        missed_test_cb: Option<Callback>,
        at_risk_cb: Option<Callback>,
        stall_detected_cb: Option<Callback>,
    ) -> (Self, thread::JoinHandle<()>) {
        let shared = Arc::new(VigilShared {
            tick_interval: atomic::AtomicUsize::new(interval_ms),
            state: atomic::AtomicUsize::new(INIT),
            terminated: atomic::AtomicBool::new(false),
	});
	let callbacks = VigilCallbacks {
            missed_test_cb,
            at_risk_cb,
            stall_detected_cb,
        };
        let thread = thread::spawn({
            let shared = shared.clone();
            move || shared.watch(callbacks)
        });

        (Vigil { shared }, thread)
    }

    /// Indicate to the vigil that the code is still active and alive.  This should be done in the
    /// same thread that is actively processing work (e.g. not in a dedicated notifier thread)
    /// otherwise deadlocks will not be caught.  If the processing thread knows it will be
    /// unavailable to notify for an extended period of time, it should use `set_interval` rather
    /// than faking up notifications.
    pub fn notify(&self) {
        self.shared.state.store(LIVE, atomic::Ordering::Relaxed);
    }

    /// Change the interval between expected notifications.  Useful if a worker thread is expecting
    /// to block on a long operation (e.g. a blocking HTTP request, or a CPU intensive
    /// calculation).  This interval will be changed until `set_interval` is called again (so code
    /// should shorten the interval once the long-blocking work is completed).
    pub fn set_interval(&self, interval_ms: usize) {
        self.shared
            .tick_interval
            .store(interval_ms, atomic::Ordering::Relaxed);
        self.notify();
    }
}

impl Drop for Vigil {
    fn drop(&mut self) {
        self.shared.terminated.store(true, atomic::Ordering::Relaxed);
    }
}

type Callback = Box<Fn() + Send + 'static>;

/// The shared state of a vigil.  This is shared between all vigil handles and the watcher thread.
struct VigilShared {
    tick_interval: atomic::AtomicUsize,
    state: atomic::AtomicUsize,
    terminated: atomic::AtomicBool,
}

/// The callbacks associated with the Vigil
struct VigilCallbacks {
    missed_test_cb: Option<Callback>,
    at_risk_cb: Option<Callback>,
    stall_detected_cb: Option<Callback>,
}

impl VigilShared {
    fn watch(&self, callbacks: VigilCallbacks) {
        loop {
            if self.terminated.load(atomic::Ordering::Relaxed) {
                info!("Vigil is terminating");
                break;
            }

            match self.state.load(atomic::Ordering::Relaxed) {
                INIT => info!("Liveness not initialized... waiting"),
                LIVE => {
                    info!("Software is live - Re-testing");
                    self.state.store(TEST, atomic::Ordering::Relaxed);
                }
                TEST => {
                    warn!("Software missed a test - Temporary glitch/slowdown?");
                    self.state
                        .compare_and_swap(TEST, RISK, atomic::Ordering::Relaxed);
                    if let Some(ref cb) = callbacks.missed_test_cb {
                        cb();
                    }
                }
                RISK => {
                    error!("Software missed multiple tests - Stall detected?");
                    self.state
                        .compare_and_swap(RISK, DEAD, atomic::Ordering::Relaxed);
                    if let Some(ref cb) = callbacks.at_risk_cb {
                        cb();
                    }
                }
                DEAD => {
                    error!("Software is still unresponsive - Likely stalled");
                    if let Some(ref cb) = callbacks.stall_detected_cb {
                        cb();
                    }
                }
                v => {
                    warn!("Liveness check had unexpected value {}, resetting", v);
                    self.state.store(INIT, atomic::Ordering::Relaxed);
                }
            }

            let interval_ms = self.tick_interval.load(atomic::Ordering::Relaxed) as u64;
            thread::sleep(Duration::from_millis(interval_ms));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_callbacks(status: Arc<atomic::AtomicUsize>) -> (Callback, Callback, Callback) {
        (
            Box::new({
                let status = status.clone();
                move || status.store(TEST, atomic::Ordering::Relaxed)
            }),
            Box::new({
                let status = status.clone();
                move || status.store(RISK, atomic::Ordering::Relaxed)
            }),
            Box::new({
                let status = status.clone();
                move || status.store(DEAD, atomic::Ordering::Relaxed)
            }),
        )
    }

    macro_rules! test {
        ($name:ident, $sleep:expr, $interval:expr, $status:expr) => {
            #[test]
            fn $name() {
                let status = Arc::new(atomic::AtomicUsize::new(INIT));
                let (a, b, c) = create_callbacks(status.clone());
                let (vigil, thread) = Vigil::create(100,
                                                    Some(a),
                                                    Some(b),
                                                    Some(c));
                for _ in 1..10 {
                    std::thread::sleep(Duration::from_millis(50));
                    vigil.notify();
                }
                vigil.set_interval($interval);
                std::thread::sleep(Duration::from_millis($sleep));
                vigil.set_interval(100);
                for _ in 1..10 {
                    std::thread::sleep(Duration::from_millis(50));
                    vigil.notify();
                }
                let status = status.load(atomic::Ordering::Relaxed);
                assert_eq!($status, status);
                drop(vigil);
                thread.join().unwrap();
            }
        };
        ($name:ident, $sleep:expr, $status:expr) => {
            test!($name, $sleep, 100, $status);
        };
    }

    test!(no_false_positives, 0, INIT);
    test!(miss_single_test, 200, TEST);
    test!(miss_multiple_tests, 300, RISK);
    test!(complete_stall, 500, DEAD);
    test!(predicted_stall, 500, 750, INIT);
}
