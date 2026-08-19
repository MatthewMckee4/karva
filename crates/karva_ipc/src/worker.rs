//! Worker-side connection and event delivery.
//!
//! The worker owns a buffered writer and a small background flusher. Lifecycle
//! checkpoints that must survive a worker crash flush synchronously; ordinary
//! events are coalesced for the configured interval.

use std::io::{BufReader, BufWriter, Write};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use karva_diagnostic::TestCaseResult;
use karva_python_semantic::{QualifiedTestName, TestCacheKey};
use serde::Serialize;

use crate::protocol::{WireMessage, WorkerEvent, WorkerSelection};
use crate::transport::{ControllerEndpoint, ControllerStream};

mod flusher;
mod frames;

use flusher::EventFlusher;
use frames::{checkpoint, completion};

/// Cloneable worker-side writer shared with execution reporters.
#[derive(Clone)]
pub struct WorkerClient {
    /// Transport shared by reporter clones created for one worker process.
    connection: Arc<WorkerConnection>,
}

/// Serialized transport and its periodic delivery worker.
struct WorkerConnection {
    /// Buffered event stream protected against concurrent reporters.
    writer: Arc<Mutex<BufWriter<ControllerStream>>>,

    /// Background delivery for events buffered between synchronous flushes.
    flusher: EventFlusher,
}

impl WorkerClient {
    /// Connects to the controller and receives this worker's owned test selection.
    pub fn connect(
        endpoint: &ControllerEndpoint,
        run_id: &str,
        worker_id: usize,
    ) -> Result<(Self, WorkerSelection)> {
        let stream = ControllerStream::connect(endpoint)?;
        stream.set_nodelay(true)?;
        let reader = stream.try_clone()?;
        let writer = Arc::new(Mutex::new(BufWriter::new(stream)));
        let flusher = EventFlusher::spawn(&writer)?;
        let client = Self {
            connection: Arc::new(WorkerConnection { writer, flusher }),
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

    /// Queues one ordinary runtime state change.
    pub fn send_event(&self, event: WorkerEvent) -> Result<()> {
        let flush = matches!(
            &event,
            WorkerEvent::TestFinished { result, .. }
                if result.outcome().is_non_success() || result.is_flaky_failure()
        );
        self.write(&WireMessage::Event(Box::new(event)), flush)
    }

    /// Queues one completed result without allocating a temporary wire event.
    pub fn send_test_finished(
        &self,
        cache_key: &TestCacheKey,
        result: &TestCaseResult,
    ) -> Result<()> {
        let flush = result.outcome().is_non_success() || result.is_flaky_failure();
        self.write(&completion(cache_key, result), flush)
    }

    /// Commits the active test identity before its setup or body can terminate the worker.
    pub fn checkpoint(&self, test_name: &QualifiedTestName) -> Result<()> {
        self.write(&checkpoint(test_name), true)
    }

    /// Marks the worker complete and gracefully closes the connection.
    pub fn complete(self) -> Result<()> {
        self.write(
            &WireMessage::Event(Box::new(WorkerEvent::WorkerFinished)),
            false,
        )?;
        self.flush()?;
        self.connection.flusher.finish()?;
        self.check_flush_error()?;
        self.lock_writer()?
            .get_ref()
            .shutdown()
            .context("failed to close Karva controller connection")
    }

    fn write(&self, message: &impl Serialize, flush: bool) -> Result<()> {
        self.check_flush_error()?;
        let mut writer = self.lock_writer()?;
        serde_json::to_writer(&mut *writer, message)
            .context("failed to serialize Karva worker event")?;
        writer
            .write_all(b"\n")
            .context("failed to frame Karva worker event")?;
        if flush {
            writer
                .flush()
                .context("failed to send Karva worker event")?;
            self.connection.flusher.mark_flushed();
        } else {
            self.connection.flusher.mark_pending();
        }
        Ok(())
    }

    /// Commits every event written so far to the controller connection.
    pub fn flush(&self) -> Result<()> {
        self.check_flush_error()?;
        let mut writer = self.lock_writer()?;
        writer
            .flush()
            .context("failed to send Karva worker event")?;
        self.connection.flusher.mark_flushed();
        Ok(())
    }

    fn check_flush_error(&self) -> Result<()> {
        self.connection.flusher.check_error()
    }

    fn lock_writer(&self) -> Result<std::sync::MutexGuard<'_, BufWriter<ControllerStream>>> {
        self.connection
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva controller connection lock poisoned"))
    }
}

/// Reads the controller's single startup response after worker authentication.
fn read_test_selection(stream: ControllerStream) -> Result<WorkerSelection> {
    let mut messages =
        serde_json::Deserializer::from_reader(BufReader::new(stream)).into_iter::<WireMessage>();
    let Some(message) = messages.next() else {
        bail!("Karva controller connection closed before sending test paths");
    };
    match message.context("failed to read Karva worker test paths")? {
        WireMessage::TestSelection(selection) => Ok(selection),
        WireMessage::Hello { .. } | WireMessage::TestCheckpoint { .. } | WireMessage::Event(_) => {
            bail!("Karva controller sent an invalid worker startup message")
        }
    }
}
