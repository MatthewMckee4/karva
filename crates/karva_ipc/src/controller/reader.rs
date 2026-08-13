//! Authentication, selection transfer, and frame intake for one connection.

use std::collections::HashMap;
use std::io::{BufReader, BufWriter, ErrorKind, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result, bail};
use crossbeam_channel::Sender;

use super::checkpoint::CheckpointState;
use super::{ControllerEvent, Incoming};
use crate::protocol::{WireMessage, WorkerEvent, WorkerSelection};
use crate::transport::ControllerStream;

/// Authenticates one stream, transfers its selection, then decodes its events.
pub(super) fn read_worker(
    stream: ControllerStream,
    expected_run_id: &str,
    worker_selections: &Mutex<HashMap<usize, WorkerSelection>>,
    checkpoint: &mut CheckpointState,
    sender: &Sender<Incoming>,
    reader_worker_id: &OnceLock<usize>,
    reader_event_count: &AtomicUsize,
) -> Result<()> {
    let response_stream = stream
        .try_clone()
        .context("failed to write Karva worker connection")?;
    let mut messages =
        serde_json::Deserializer::from_reader(BufReader::new(stream)).into_iter::<WireMessage>();
    let Some(first) = messages.next() else {
        // The process supervisor owns diagnostics and recovery when a worker
        // exits after connecting but before identifying itself.
        return Ok(());
    };
    let first = match first {
        Ok(first) => first,
        Err(error) if error.is_eof() || is_clean_disconnect(&error) => return Ok(()),
        Err(error) => return Err(error).context("failed to read Karva worker handshake"),
    };
    let WireMessage::Hello { run_id, worker_id } = first else {
        bail!("Karva worker sent an event before its handshake");
    };
    if run_id != expected_run_id {
        bail!("Karva worker connected with run id `{run_id}`, expected `{expected_run_id}`");
    }
    reader_worker_id
        .set(worker_id)
        .map_err(|_| anyhow::anyhow!("Karva worker id was already authenticated"))?;
    let selection = worker_selections
        .lock()
        .map_err(|_| anyhow::anyhow!("Karva worker selection lock poisoned"))?
        .remove(&worker_id)
        .with_context(|| {
            format!("Karva worker {worker_id} connected without a registered selection")
        })?;
    let mut writer = BufWriter::new(response_stream);
    let selection_result =
        serde_json::to_writer(&mut writer, &WireMessage::TestSelection(selection))
            .context("failed to serialize Karva worker selection")
            .and_then(|()| {
                writer
                    .write_all(b"\n")
                    .context("failed to frame Karva worker selection")?;
                writer
                    .flush()
                    .context("failed to send Karva worker selection")
            });
    if selection_result.is_err() {
        return Ok(());
    }
    if sender.send(Incoming::Connected { worker_id }).is_err() {
        return Ok(());
    }

    let mut event_count = 0_usize;
    for message in messages {
        let message = match message {
            Ok(message) => message,
            Err(error) if error.is_eof() || is_clean_disconnect(&error) => break,
            Err(error) => return Err(error).context("failed to read Karva worker event"),
        };
        let event = match message {
            WireMessage::TestCheckpoint {
                parameters,
                cache_key,
            } => {
                checkpoint.record(worker_id, parameters, cache_key)?;
                event_count = event_count.saturating_add(1);
                reader_event_count.store(event_count, Ordering::Relaxed);
                continue;
            }
            WireMessage::Event(event) => event,
            WireMessage::Hello { .. } => bail!("Karva worker sent more than one handshake"),
            WireMessage::TestSelection(_) => {
                bail!("Karva worker sent a test selection to its controller")
            }
        };
        match event.as_ref() {
            WorkerEvent::TestFinished { cache_key, .. } => {
                checkpoint.complete(worker_id, cache_key)?;
            }
            WorkerEvent::WorkerFinished => {
                checkpoint.ensure_idle(worker_id)?;
            }
            WorkerEvent::TestSlow | WorkerEvent::RunDiagnostic(_) => {}
        }
        if sender
            .send(Incoming::Event(ControllerEvent { worker_id, event }))
            .is_err()
        {
            return Ok(());
        }
        event_count = event_count.saturating_add(1);
        reader_event_count.store(event_count, Ordering::Relaxed);
    }
    Ok(())
}

/// Distinguishes normal socket teardown from a malformed event frame.
pub(super) fn is_clean_disconnect(error: &serde_json::Error) -> bool {
    matches!(
        error.io_error_kind(),
        Some(ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted)
    )
}
