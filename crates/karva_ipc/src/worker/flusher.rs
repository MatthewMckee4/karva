//! Periodic delivery for worker events buffered between synchronous flushes.

use std::io::{BufWriter, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result, bail};

// Receipt: PR 1268 at 5a8d65a was 39.6% slower than main 7c270fa on the
// 16,807-case parametrized CI workload. Checkpoints flush before fixture setup
// and the test body; successful results buffer until the next checkpoint or
// broader-scope teardown, while failures flush immediately for fail-fast.
const EVENT_FLUSH_INTERVAL: Duration = Duration::from_millis(10);

/// Background flush thread and its shared control state.
pub(super) struct EventFlusher {
    /// Signals shared with the worker thread without taking the writer lock.
    signals: Arc<FlusherSignals>,

    /// Join handle retained so shutdown can report a flusher panic.
    handle: Mutex<Option<JoinHandle<()>>>,
}

/// Atomic scheduling and error state shared with the flusher thread.
struct FlusherSignals {
    /// Whether serialized events are waiting in the writer buffer.
    pending: AtomicBool,

    /// Whether connection shutdown has asked the flusher to exit.
    stop: AtomicBool,

    /// Fast-path signal avoiding an error mutex lock for healthy writes.
    failed: AtomicBool,

    /// First asynchronous delivery failure.
    error: Mutex<Option<String>>,
}

impl EventFlusher {
    /// Starts one low-frequency flusher for a worker connection.
    pub(super) fn spawn(writer: &Arc<Mutex<BufWriter<TcpStream>>>) -> Result<Self> {
        let writer = Arc::clone(writer);
        let signals = Arc::new(FlusherSignals {
            pending: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            error: Mutex::new(None),
        });
        let thread_signals = Arc::clone(&signals);
        let handle = thread::Builder::new()
            .name("karva-event-flusher".to_string())
            .spawn(move || run(&writer, &thread_signals))
            .context("failed to start Karva worker event flusher")?;
        Ok(Self {
            signals,
            handle: Mutex::new(Some(handle)),
        })
    }

    /// Marks buffered bytes for periodic delivery.
    pub(super) fn mark_pending(&self) {
        self.signals.pending.store(true, Ordering::Release);
    }

    /// Clears the pending signal after synchronous delivery.
    pub(super) fn mark_flushed(&self) {
        self.signals.pending.store(false, Ordering::Release);
    }

    /// Returns the first asynchronous delivery error, when present.
    pub(super) fn check_error(&self) -> Result<()> {
        if !self.signals.failed.load(Ordering::Acquire) {
            return Ok(());
        }
        if let Some(error) = self
            .signals
            .error
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))?
            .as_ref()
        {
            bail!("failed to send Karva worker event: {error}");
        }
        Ok(())
    }

    /// Stops and joins the flusher, reporting a thread panic.
    pub(super) fn finish(&self) -> Result<()> {
        let handle = self
            .handle
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))?
            .take();
        if stop_and_join(&self.signals, handle).is_err() {
            bail!("Karva worker event flusher panicked");
        }
        Ok(())
    }
}

/// Flushes pending bytes until connection shutdown or the first error.
fn run(writer: &Mutex<BufWriter<TcpStream>>, signals: &FlusherSignals) {
    while !signals.stop.load(Ordering::Acquire) {
        thread::park_timeout(EVENT_FLUSH_INTERVAL);
        if signals.stop.load(Ordering::Acquire) {
            return;
        }
        if let Err(error) = flush_pending(writer, signals) {
            if let Ok(mut recorded) = signals.error.lock() {
                *recorded = Some(error);
            }
            signals.failed.store(true, Ordering::Release);
            return;
        }
    }
}

/// Flushes pending bytes without losing writes racing with the periodic check.
///
/// The pending flag clears only while holding the writer lock. A later writer
/// must acquire the same lock before setting it again.
fn flush_pending(
    writer: &Mutex<BufWriter<TcpStream>>,
    signals: &FlusherSignals,
) -> Result<(), String> {
    if !signals.pending.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut writer = writer
        .lock()
        .map_err(|_| "Karva controller connection lock poisoned".to_string())?;
    if signals.pending.swap(false, Ordering::AcqRel) {
        writer.flush().map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Signals shutdown and joins an optional flusher handle exactly once.
fn stop_and_join(signals: &FlusherSignals, handle: Option<JoinHandle<()>>) -> thread::Result<()> {
    signals.stop.store(true, Ordering::Release);
    let Some(handle) = handle else {
        return Ok(());
    };
    handle.thread().unpark();
    handle.join()
}

impl Drop for EventFlusher {
    #[expect(
        clippy::print_stderr,
        reason = "Drop cannot return the background thread's panic"
    )]
    fn drop(&mut self) {
        if let Ok(handle) = self.handle.get_mut()
            && let Err(error) = stop_and_join(&self.signals, handle.take())
        {
            eprintln!("Karva worker event flusher panicked during cleanup: {error:?}");
        }
    }
}
