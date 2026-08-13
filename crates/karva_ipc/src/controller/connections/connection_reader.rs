//! One accepted controller connection and its reader-thread controls.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result};
use crossbeam_channel::Sender;

use super::super::Incoming;
use super::super::checkpoint::{CheckpointState, WorkerCheckpoint};
use super::super::reader::read_worker;
use crate::protocol::WorkerSelection;

/// Reader thread and shared progress state for one worker connection.
pub(super) struct ControllerReader {
    /// Thread consuming and authenticating the worker stream.
    handle: Option<JoinHandle<()>>,

    /// Controller clone used to interrupt an escaped descendant's socket.
    stream: TcpStream,

    /// Worker id populated after the handshake succeeds.
    worker_id: Arc<OnceLock<usize>>,

    /// Number of complete event frames consumed by the reader.
    event_count: Arc<AtomicUsize>,

    /// Final active checkpoint published before disconnection is announced.
    checkpoint: Arc<Mutex<Option<WorkerCheckpoint>>>,
}

impl ControllerReader {
    /// Starts a reader for one accepted socket and retains its control handles.
    pub(super) fn spawn(
        stream: TcpStream,
        run_id: String,
        sender: Sender<Incoming>,
        worker_selections: Arc<Mutex<HashMap<usize, WorkerSelection>>>,
    ) -> Result<Self> {
        stream
            .set_nonblocking(false)
            .context("failed to configure Karva worker connection")?;
        let control_stream = stream
            .try_clone()
            .context("failed to control Karva worker connection")?;
        let shutdown_stream = stream
            .try_clone()
            .context("failed to close Karva worker connection")?;
        let worker_id = Arc::new(OnceLock::new());
        let reader_worker_id = Arc::clone(&worker_id);
        let event_count = Arc::new(AtomicUsize::new(0));
        let reader_event_count = Arc::clone(&event_count);
        let checkpoint = Arc::new(Mutex::new(None));
        let published_checkpoint = Arc::clone(&checkpoint);
        let handle = thread::spawn(move || {
            let mut checkpoint = CheckpointState::default();
            let result = read_worker(
                stream,
                &run_id,
                &worker_selections,
                &mut checkpoint,
                &sender,
                &reader_worker_id,
                &reader_event_count,
            );
            let publish_result = published_checkpoint
                .lock()
                .map(|mut published| *published = checkpoint.into_checkpoint())
                .map_err(|_| anyhow::anyhow!("Karva worker checkpoint lock poisoned"));
            if let Err(error) = result.and(publish_result) {
                sender.send(Incoming::Error(format!("{error:#}"))).ok();
            } else if let Some(worker_id) = reader_worker_id.get().copied() {
                sender.send(Incoming::Disconnected { worker_id }).ok();
            }
            shutdown_stream.shutdown(Shutdown::Write).ok();
            shutdown_stream.shutdown(Shutdown::Read).ok();
        });
        Ok(Self {
            handle: Some(handle),
            stream: control_stream,
            worker_id,
            event_count,
            checkpoint,
        })
    }

    /// Returns whether this reader belongs to a worker id.
    pub(super) fn has_worker_id(&self, worker_id: usize) -> bool {
        self.worker_id.get().is_some_and(|id| *id == worker_id)
    }

    /// Returns the number of complete event frames read by this reader.
    pub(super) fn event_count(&self) -> usize {
        self.event_count.load(Ordering::Relaxed)
    }

    /// Removes this reader's final checkpoint after it has disconnected.
    ///
    /// The caller must first join the reader or observe its FIFO
    /// `Disconnected` notification so publication is complete.
    pub(super) fn take_checkpoint(&self, worker_id: usize) -> Result<Option<WorkerCheckpoint>> {
        Ok(self
            .checkpoint
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva worker {worker_id} checkpoint lock poisoned"))?
            .take())
    }

    /// Interrupts this reader, tolerating a stream already closed by its peer.
    pub(super) fn disconnect(&self) -> Result<()> {
        shutdown_reader(&self.stream)
    }

    /// Joins this reader thread and reports a panic without losing the join.
    pub(super) fn finish(&mut self) -> bool {
        self.handle
            .take()
            .is_some_and(|handle| handle.join().is_err())
    }
}

/// Interrupts one reader thread, tolerating a stream already closed by its peer.
fn shutdown_reader(stream: &TcpStream) -> Result<()> {
    stream
        .shutdown(Shutdown::Both)
        .or_else(|error| {
            if error.kind() == ErrorKind::NotConnected {
                Ok(())
            } else {
                Err(error)
            }
        })
        .context("failed to close Karva worker connection")
}
