//! Controller-side listener, authentication, and worker event intake.
//!
//! Each accepted connection gets a reader thread. The reader authenticates the
//! worker, sends its selection, keeps the latest crash checkpoint in a bounded
//! mailbox, and forwards result events through a channel. The runner remains
//! responsible for serial result aggregation.

mod checkpoint;
mod connections;
mod reader;

use std::collections::HashSet;

use anyhow::Result;
use crossbeam_channel::{Receiver, TryRecvError, unbounded};

use crate::protocol::{WorkerEvent, WorkerSelection};
use crate::transport::ControllerEndpoint;
pub use checkpoint::WorkerCheckpoint;
use connections::ControllerConnections;
#[cfg(test)]
use reader::is_clean_disconnect;

/// Reader-thread notification consumed by the controller event loop.
enum Incoming {
    /// Worker completed authentication.
    Connected { worker_id: usize },

    /// Worker event stream reached a clean end.
    Disconnected { worker_id: usize },

    /// Authenticated runtime event ready for serial dispatch.
    Event(ControllerEvent),

    /// Reader failure that must terminate the controller run.
    Error(String),
}

/// Event attributed to the worker authenticated by the stream handshake.
pub struct ControllerEvent {
    /// Worker number established by the connection handshake.
    pub worker_id: usize,

    /// Runtime state change received from that worker.
    pub event: Box<WorkerEvent>,
}

/// How a targeted worker reader reached its terminal state.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum WorkerConnectionClose {
    /// No interrupting shutdown was needed because the reader had finished or was absent.
    Complete,

    /// The controller interrupted a reader still held open past its drain limit.
    Forced,
}

/// Controller-side event loop and worker lifecycle state.
pub struct ControllerServer {
    /// Listener, selections, readers, and stream shutdown state.
    connections: ControllerConnections,

    /// Event queue and per-run worker lifecycle sets.
    events: ControllerEvents,
}

/// Controller event queue and authentication/disconnect state.
struct ControllerEvents {
    /// Controller-side queue of reader notifications.
    receiver: Receiver<Incoming>,

    /// Workers that completed authentication.
    workers: HashSet<usize>,

    /// Workers whose event streams reached EOF.
    disconnected_workers: HashSet<usize>,
}

impl ControllerEvents {
    /// Creates controller-owned event state and its reader notification sender.
    ///
    /// `ControllerConnections` owns the returned sender and clones it into
    /// reader threads; this state deliberately retains only the receiving end.
    fn new() -> (Self, crossbeam_channel::Sender<Incoming>) {
        let (sender, receiver) = unbounded();
        (
            Self {
                receiver,
                workers: HashSet::new(),
                disconnected_workers: HashSet::new(),
            },
            sender,
        )
    }

    /// Consumes queued reader notifications until one result event is ready.
    fn try_recv(&mut self) -> Result<Option<ControllerEvent>> {
        loop {
            match self.receiver.try_recv() {
                Ok(Incoming::Connected { worker_id }) => {
                    if !self.workers.insert(worker_id) {
                        anyhow::bail!("Karva worker {worker_id} connected more than once");
                    }
                }
                Ok(Incoming::Disconnected { worker_id }) => {
                    self.disconnected_workers.insert(worker_id);
                }
                Ok(Incoming::Event(event)) => return Ok(Some(event)),
                Ok(Incoming::Error(error)) => anyhow::bail!(error),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(None),
            }
        }
    }
}

impl ControllerServer {
    /// Binds an operating-system-selected local endpoint for one test run.
    pub fn bind(run_id: &str) -> Result<Self> {
        let (events, sender) = ControllerEvents::new();
        Ok(Self {
            connections: ControllerConnections::bind(run_id, sender)?,
            events,
        })
    }

    /// Registers one worker's owned test selection before spawning it.
    ///
    /// Ownership moves into the connection reader so large selections are not
    /// cloned or collected into an encoded buffer on the controller.
    pub fn register_worker_selection(
        &mut self,
        worker_id: usize,
        selection: WorkerSelection,
    ) -> Result<()> {
        self.connections
            .register_worker_selection(worker_id, selection)
    }

    /// Returns the local endpoint passed to newly spawned workers.
    pub fn endpoint(&self) -> ControllerEndpoint {
        self.connections.endpoint()
    }

    /// Accepts every connection already queued by the operating system.
    pub fn accept_pending(&mut self) -> Result<()> {
        self.connections.accept_pending()
    }

    /// Returns next queued worker event without blocking.
    pub fn try_recv(&mut self) -> Result<Option<ControllerEvent>> {
        self.events.try_recv()
    }

    /// Whether the worker's event stream reached EOF after every queued event.
    pub fn worker_disconnected(&self, worker_id: usize) -> bool {
        self.events.disconnected_workers.contains(&worker_id)
    }

    /// Whether the worker began or completed its controller handshake.
    pub fn worker_started(&self, worker_id: usize) -> Result<bool> {
        if self.events.workers.contains(&worker_id) {
            return Ok(true);
        }
        Ok(!self.connections.selection_pending(worker_id)?)
    }

    /// Number of complete event frames read from one authenticated worker.
    pub fn worker_event_count(&self, worker_id: usize) -> usize {
        self.connections.worker_event_count(worker_id)
    }

    /// Removes a disconnected generation's final checkpoint for recovery.
    ///
    /// Call only after [`Self::worker_disconnected`] returns true,
    /// [`Self::close_worker_connection`] joins this reader, or [`Self::finish`]
    /// joins every reader. Each condition guarantees checkpoint publication.
    pub fn take_worker_checkpoint(&self, worker_id: usize) -> Result<Option<WorkerCheckpoint>> {
        self.connections.take_worker_checkpoint(worker_id)
    }

    /// Closes and joins one authenticated worker reader.
    ///
    /// This publishes every frame decoded before closure and the reader's final
    /// active checkpoint before returning, without waiting for unrelated readers.
    /// The return value distinguishes a reader that had already stopped from
    /// one interrupted by this call.
    pub fn close_worker_connection(&mut self, worker_id: usize) -> Result<WorkerConnectionClose> {
        self.connections.close_worker_connection(worker_id)
    }

    /// Closes every accepted reader after controller-driven worker termination.
    ///
    /// Callers must first stop the worker processes and finish any late-event
    /// drain they require. Closing the controller clones prevents escaped
    /// descendants from keeping reader threads alive indefinitely.
    pub fn disconnect_readers(&self) -> Result<()> {
        self.connections.disconnect_readers()
    }

    /// Joins every accepted reader after worker processes have exited.
    pub fn finish(&mut self) -> Result<()> {
        self.connections.finish()
    }

    /// Number of accepted reader connections used by lifecycle tests.
    #[cfg(test)]
    fn reader_count(&self) -> usize {
        self.connections.reader_count()
    }

    /// Whether one authenticated reader thread has already stopped.
    #[cfg(test)]
    fn worker_reader_finished(&self, worker_id: usize) -> bool {
        self.connections.worker_reader_finished(worker_id)
    }
}

#[cfg(test)]
mod tests;
