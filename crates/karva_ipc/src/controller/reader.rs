//! Authentication, selection transfer, and frame intake for one connection.

use std::collections::HashMap;
use std::io::{BufReader, BufWriter, ErrorKind, Write};
use std::net::TcpStream;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, bail};
use crossbeam_channel::Sender;

use super::checkpoint::CheckpointState;
use super::{ControllerEvent, Incoming};
use crate::protocol::{WireMessage, WorkerEvent, WorkerSelection};

/// Authenticates one stream, transfers its selection, then decodes its events.
pub(super) fn read_worker(
    stream: TcpStream,
    expected_run_id: &str,
    worker_selections: &Mutex<HashMap<usize, WorkerSelection>>,
    checkpoint: &mut CheckpointState,
    sender: &Sender<Incoming>,
    reader_worker_id: &AtomicUsize,
    reader_event_count: &AtomicUsize,
) -> Result<()> {
    let response_stream = stream
        .try_clone()
        .context("failed to write Karva worker connection")?;
    let mut messages =
        serde_json::Deserializer::from_reader(BufReader::new(stream)).into_iter::<WireMessage>();
    let Some(first) = messages.next() else {
        bail!("Karva worker connection closed before handshake");
    };
    let WireMessage::Hello { run_id, worker_id } =
        first.context("failed to read Karva worker handshake")?
    else {
        bail!("Karva worker sent an event before its handshake");
    };
    if run_id != expected_run_id {
        bail!("Karva worker connected with run id `{run_id}`, expected `{expected_run_id}`");
    }
    reader_worker_id.store(worker_id, Ordering::Release);
    let selection = worker_selections
        .lock()
        .map_err(|_| anyhow::anyhow!("Karva worker selection lock poisoned"))?
        .remove(&worker_id)
        .with_context(|| {
            format!("Karva worker {worker_id} connected without a registered selection")
        })?;
    if sender.send(Incoming::Connected { worker_id }).is_err() {
        return Ok(());
    }
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
        let incoming = match *event {
            WorkerEvent::TestFinished { cache_key, result } => {
                checkpoint.complete(worker_id, &cache_key)?;
                Some(WorkerEvent::TestFinished { cache_key, result })
            }
            WorkerEvent::WorkerFinished => {
                checkpoint.ensure_idle(worker_id)?;
                Some(WorkerEvent::WorkerFinished)
            }
            WorkerEvent::TestSlow => Some(WorkerEvent::TestSlow),
            WorkerEvent::RunDiagnostic(diagnostic) => Some(WorkerEvent::RunDiagnostic(diagnostic)),
        };
        if let Some(event) = incoming
            && sender
                .send(Incoming::Event(ControllerEvent {
                    worker_id,
                    event: Box::new(event),
                }))
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
