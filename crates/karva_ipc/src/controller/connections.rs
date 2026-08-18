//! Lifecycle control for controller-side worker connection readers.
//!
//! The controller event loop owns protocol state, while this module owns the
//! operating-system resources associated with each accepted connection:
//! reader threads, interruptible stream clones, and reader progress counters.

mod connection_reader;

use std::collections::HashMap;
use std::io::ErrorKind;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use crossbeam_channel::Sender;

use super::{Incoming, WorkerConnectionClose};
use crate::protocol::WorkerSelection;
use crate::transport::{ControllerEndpoint, ControllerListener};
use connection_reader::ControllerReader;

/// Operating-system resources and connection state for one controller run.
///
/// Keeping listener setup, selection ownership, reader threads, and shutdown
/// handles together makes their lifetime contract explicit. The owning server
/// calls [`ControllerConnections::finish`] to surface reader failures; drop is
/// a best-effort fallback that closes and joins every remaining reader.
pub(super) struct ControllerConnections {
    /// Non-blocking listener used to accept worker connections.
    listener: ControllerListener,

    /// Run identifier required in every worker handshake.
    run_id: String,

    /// Reader-to-controller notification sender cloned into each reader.
    sender: Sender<Incoming>,

    /// Reader threads and control streams retained until run shutdown.
    readers: Vec<ControllerReader>,

    /// Selections waiting for their authenticated worker connection.
    worker_selections: Arc<Mutex<HashMap<usize, WorkerSelection>>>,
}

impl ControllerConnections {
    /// Binds an operating-system-selected local endpoint for one test run.
    pub(super) fn bind(run_id: &str, sender: Sender<Incoming>) -> Result<Self> {
        let listener = ControllerListener::bind()?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            run_id: run_id.to_string(),
            sender,
            readers: Vec::new(),
            worker_selections: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Registers one worker's owned test selection before spawning it.
    pub(super) fn register_worker_selection(
        &self,
        worker_id: usize,
        selection: WorkerSelection,
    ) -> Result<()> {
        let previous = self
            .worker_selections
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva worker selection lock poisoned"))?
            .insert(worker_id, selection);
        if previous.is_some() {
            anyhow::bail!("Karva worker {worker_id} selection registered more than once");
        }
        Ok(())
    }

    /// Returns the endpoint passed to newly spawned workers.
    pub(super) fn endpoint(&self) -> ControllerEndpoint {
        self.listener.endpoint()
    }

    /// Accepts all worker connections currently queued by the operating system.
    pub(super) fn accept_pending(&mut self) -> Result<()> {
        loop {
            match self.listener.accept() {
                Ok(stream) => self.readers.push(ControllerReader::spawn(
                    stream,
                    self.run_id.clone(),
                    self.sender.clone(),
                    Arc::clone(&self.worker_selections),
                )?),
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(error) => {
                    return Err(error).context("failed to accept Karva worker connection");
                }
            }
        }
    }

    /// Returns whether a worker still has a selection waiting for its handshake.
    pub(super) fn selection_pending(&self, worker_id: usize) -> Result<bool> {
        self.worker_selections
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva worker selection lock poisoned"))
            .map(|selections| selections.contains_key(&worker_id))
    }

    /// Number of complete event frames read from one authenticated worker.
    pub(super) fn worker_event_count(&self, worker_id: usize) -> usize {
        self.readers
            .iter()
            .find(|reader| reader.has_worker_id(worker_id))
            .map_or(0, ControllerReader::event_count)
    }

    /// Removes a disconnected generation and returns its final checkpoint.
    pub(super) fn take_worker_checkpoint(
        &mut self,
        worker_id: usize,
    ) -> Result<Option<super::checkpoint::WorkerCheckpoint>> {
        let Some(position) = self
            .readers
            .iter()
            .position(|reader| reader.has_worker_id(worker_id))
        else {
            return Ok(None);
        };
        let mut reader = self.readers.swap_remove(position);
        if reader.finish() {
            anyhow::bail!("Karva worker {worker_id} connection reader panicked");
        }
        reader.take_checkpoint(worker_id)
    }

    /// Closes and joins one authenticated worker reader.
    ///
    /// Joining guarantees that its final decoded checkpoint state is published
    /// before crash recovery inspects it. Other worker readers remain active.
    pub(super) fn close_worker_connection(
        &mut self,
        worker_id: usize,
    ) -> Result<WorkerConnectionClose> {
        let Some(reader) = self
            .readers
            .iter_mut()
            .find(|reader| reader.has_worker_id(worker_id))
        else {
            return Ok(WorkerConnectionClose::Complete);
        };
        let (close, disconnect_result) = if reader.is_finished() {
            (WorkerConnectionClose::Complete, Ok(()))
        } else {
            (WorkerConnectionClose::Forced, reader.disconnect())
        };
        if reader.finish() {
            anyhow::bail!("Karva worker {worker_id} connection reader panicked");
        }
        disconnect_result?;
        Ok(close)
    }

    /// Closes every accepted reader after controller-driven worker termination.
    pub(super) fn disconnect_readers(&self) -> Result<()> {
        let mut first_error = None;
        for reader in &self.readers {
            if let Err(error) = reader.disconnect()
                && first_error.is_none()
            {
                first_error = Some(error);
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(())
    }

    /// Joins every accepted reader after worker processes have exited.
    pub(super) fn finish(&mut self) -> Result<()> {
        self.accept_pending()?;
        let mut reader_panicked = false;
        for reader in &mut self.readers {
            if reader.finish() {
                reader_panicked = true;
            }
        }
        if reader_panicked {
            anyhow::bail!("Karva worker connection reader panicked");
        }
        Ok(())
    }

    /// Number of accepted readers, used by connection lifecycle tests.
    #[cfg(test)]
    pub(super) fn reader_count(&self) -> usize {
        self.readers.len()
    }

    /// Whether one authenticated reader thread has already stopped.
    #[cfg(test)]
    pub(super) fn worker_reader_finished(&self, worker_id: usize) -> bool {
        self.readers
            .iter()
            .find(|reader| reader.has_worker_id(worker_id))
            .is_some_and(ControllerReader::is_finished)
    }
}

impl Drop for ControllerConnections {
    fn drop(&mut self) {
        if let Err(error) = self.disconnect_readers() {
            tracing::warn!(
                "failed to close Karva worker connection during controller cleanup: {error}"
            );
        }
        let mut reader_panicked = false;
        for reader in &mut self.readers {
            reader_panicked |= reader.finish();
        }
        if reader_panicked {
            tracing::warn!("Karva worker connection reader panicked during controller cleanup");
        }
    }
}
