//! Worker-side connection and event delivery.
//!
//! The worker owns a buffered writer and a small background flusher. Lifecycle
//! checkpoints that must survive a worker crash flush synchronously; ordinary
//! events are coalesced for the configured interval.

use std::io::{BufReader, BufWriter, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::protocol::{WireMessage, WorkerEvent, WorkerSelection};

/// Cloneable worker-side writer shared with execution reporters.
#[derive(Clone)]
pub struct WorkerClient {
    /// Transport shared by reporter clones created for one worker process.
    connection: Arc<WorkerConnection>,
}

/// Shared writer, flush scheduling, and shutdown state for one connection.
struct WorkerConnection {
    /// Serialized event stream protected against concurrent reporters.
    writer: Arc<Mutex<BufWriter<TcpStream>>>,

    /// Whether the periodic flusher has work waiting in the writer buffer.
    pending_flush: Arc<AtomicBool>,

    /// Signals the periodic flusher to stop during connection shutdown.
    stop: Arc<AtomicBool>,

    /// First asynchronous flush failure observed by the flusher thread.
    flush_error: Arc<Mutex<Option<String>>>,

    /// Fast-path signal avoiding an error mutex lock for healthy writes.
    flush_failed: Arc<AtomicBool>,

    /// Join handle retained so completion can report a flusher panic.
    flusher: Mutex<Option<JoinHandle<()>>>,
}

impl WorkerClient {
    /// Connects to the controller and receives this worker's owned test selection.
    pub fn connect(
        address: SocketAddr,
        run_id: &str,
        worker_id: usize,
    ) -> Result<(Self, WorkerSelection)> {
        let stream = TcpStream::connect(address)
            .with_context(|| format!("failed to connect to Karva controller at {address}"))?;
        stream
            .set_nodelay(true)
            .context("failed to configure Karva controller connection")?;
        let reader = stream
            .try_clone()
            .context("failed to read Karva controller connection")?;
        let writer = Arc::new(Mutex::new(BufWriter::new(stream)));
        let pending_flush = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let flush_error = Arc::new(Mutex::new(None));
        let flush_failed = Arc::new(AtomicBool::new(false));
        let flusher = spawn_flusher(&writer, &pending_flush, &stop, &flush_error, &flush_failed)?;
        let client = Self {
            connection: Arc::new(WorkerConnection {
                writer,
                pending_flush,
                stop,
                flush_error,
                flush_failed,
                flusher: Mutex::new(Some(flusher)),
            }),
        };
        client.write(
            &WireMessage::Hello {
                run_id: run_id.to_string(),
                worker_id,
            },
            true,
        )?;
        let selection = read_test_selection(reader)?;
        Ok((client, selection))
    }

    /// Queues one state change, synchronously committing crash checkpoints.
    pub fn send_event(&self, event: WorkerEvent) -> Result<()> {
        let flush = matches!(&event, WorkerEvent::TestStarted { .. })
            || matches!(
                &event,
                WorkerEvent::TestFinished { result, .. } if result.outcome().is_non_success()
            );
        self.write(&WireMessage::Event(Box::new(event)), flush)
    }

    /// Marks the worker complete and gracefully closes the connection.
    pub fn complete(self) -> Result<()> {
        self.write(
            &WireMessage::Event(Box::new(WorkerEvent::WorkerFinished)),
            false,
        )?;
        self.flush()?;
        self.connection.stop.store(true, Ordering::Release);
        let flusher = self
            .connection
            .flusher
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))?
            .take();
        if let Some(flusher) = flusher {
            flusher.thread().unpark();
            if flusher.join().is_err() {
                bail!("Karva worker event flusher panicked");
            }
        }
        self.check_flush_error()?;
        self.connection
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))?
            .get_ref()
            .shutdown(Shutdown::Both)
            .context("failed to close Karva controller connection")
    }

    fn write(&self, message: &WireMessage, flush: bool) -> Result<()> {
        self.check_flush_error()?;
        let mut writer = self
            .connection
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))?;
        serde_json::to_writer(&mut *writer, message)
            .context("failed to serialize Karva worker event")?;
        writer
            .write_all(b"\n")
            .context("failed to frame Karva worker event")?;
        if flush {
            writer
                .flush()
                .context("failed to send Karva worker event")?;
            self.connection
                .pending_flush
                .store(false, Ordering::Release);
        } else {
            self.connection.pending_flush.store(true, Ordering::Release);
        }
        Ok(())
    }

    /// Commits every event written so far to the controller connection.
    pub fn flush(&self) -> Result<()> {
        self.check_flush_error()?;
        let mut writer = self
            .connection
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))?;
        writer
            .flush()
            .context("failed to send Karva worker event")?;
        self.connection
            .pending_flush
            .store(false, Ordering::Release);
        Ok(())
    }

    fn check_flush_error(&self) -> Result<()> {
        if !self.connection.flush_failed.load(Ordering::Acquire) {
            return Ok(());
        }
        if let Some(error) = self
            .connection
            .flush_error
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))?
            .as_ref()
        {
            bail!("failed to send Karva worker event: {error}");
        }
        Ok(())
    }
}

/// Reads the controller's single startup response after worker authentication.
fn read_test_selection(stream: TcpStream) -> Result<WorkerSelection> {
    let mut messages =
        serde_json::Deserializer::from_reader(BufReader::new(stream)).into_iter::<WireMessage>();
    let Some(message) = messages.next() else {
        bail!("Karva controller connection closed before sending test paths");
    };
    match message.context("failed to read Karva worker test paths")? {
        WireMessage::TestSelection(selection) => Ok(selection),
        WireMessage::Hello { .. } | WireMessage::Event(_) => {
            bail!("Karva controller sent an invalid worker startup message")
        }
    }
}

// Receipt: PR 1268 at 5a8d65a was 39.6% slower than main 7c270fa on the
// 16,807-case parametrized CI workload. Starts flush before fixture setup and
// the test body; successful results buffer until the next checkpoint or
// broader-scope teardown, while failures flush immediately for fail-fast.
const EVENT_FLUSH_INTERVAL: Duration = Duration::from_millis(10);

/// Starts the low-frequency writer that bounds delivery latency for buffered events.
fn spawn_flusher(
    writer: &Arc<Mutex<BufWriter<TcpStream>>>,
    pending_flush: &Arc<AtomicBool>,
    stop: &Arc<AtomicBool>,
    flush_error: &Arc<Mutex<Option<String>>>,
    flush_failed: &Arc<AtomicBool>,
) -> Result<JoinHandle<()>> {
    let writer = Arc::clone(writer);
    let pending_flush = Arc::clone(pending_flush);
    let stop = Arc::clone(stop);
    let flush_error = Arc::clone(flush_error);
    let flush_failed = Arc::clone(flush_failed);
    thread::Builder::new()
        .name("karva-event-flusher".to_string())
        .spawn(move || {
            while !stop.load(Ordering::Acquire) {
                thread::park_timeout(EVENT_FLUSH_INTERVAL);
                if stop.load(Ordering::Acquire) {
                    return;
                }
                let result = flush_worker_events(&writer, &pending_flush);
                if let Err(error) = result {
                    if let Ok(mut flush_error) = flush_error.lock() {
                        *flush_error = Some(error);
                    }
                    flush_failed.store(true, Ordering::Release);
                    return;
                }
            }
        })
        .context("failed to start Karva worker event flusher")
}

/// Flushes pending bytes without losing writes racing with the periodic check.
///
/// The flag is cleared only while holding the writer lock. A writer that runs
/// after the swap must first acquire the same lock, then sets the flag again.
fn flush_worker_events(
    writer: &Mutex<BufWriter<TcpStream>>,
    pending_flush: &AtomicBool,
) -> Result<(), String> {
    if !pending_flush.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut writer = writer
        .lock()
        .map_err(|_| "Karva controller connection lock poisoned".to_string())?;
    if pending_flush.swap(false, Ordering::AcqRel) {
        writer.flush().map_err(|error| error.to_string())?;
    }
    Ok(())
}

impl Drop for WorkerConnection {
    #[expect(
        clippy::print_stderr,
        reason = "Drop cannot return the background thread's panic"
    )]
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Ok(flusher) = self.flusher.get_mut()
            && let Some(flusher) = flusher.take()
        {
            flusher.thread().unpark();
            if let Err(error) = flusher.join() {
                eprintln!("Karva worker event flusher panicked during cleanup: {error:?}");
            }
        }
    }
}
