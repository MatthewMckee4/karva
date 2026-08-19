//! Authentication, selection transfer, and frame intake for one connection.

use std::collections::HashMap;
use std::io::{BufReader, BufWriter, ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result, bail};
use crossbeam_channel::Sender;

use super::checkpoint::CheckpointState;
use super::connections::RegisteredWorkerSelection;
use super::{ControllerEvent, Incoming};
use crate::protocol::{WireMessage, WorkerEvent};
use crate::transport::ControllerStream;

/// Authenticates one stream, transfers its selection, then decodes its events.
pub(super) fn read_worker(
    reader: InterruptibleReader<'_, ControllerStream>,
    expected_run_id: &str,
    worker_selections: &Mutex<HashMap<usize, RegisteredWorkerSelection>>,
    checkpoint: &mut CheckpointState,
    sender: &Sender<Incoming>,
    reader_worker_id: &OnceLock<usize>,
    reader_event_count: &AtomicUsize,
) -> Result<()> {
    let response_stream = reader
        .stream
        .try_clone()
        .context("failed to write Karva worker connection")?;
    let mut messages =
        serde_json::Deserializer::from_reader(BufReader::new(reader)).into_iter::<WireMessage>();
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
    let selection = {
        let mut registrations = worker_selections
            .lock()
            .map_err(|_| anyhow::anyhow!("Karva worker selection lock poisoned"))?;
        if matches!(
            registrations.get(&worker_id),
            Some(RegisteredWorkerSelection::Retired)
        ) {
            return Ok(());
        }
        match registrations.remove(&worker_id) {
            Some(RegisteredWorkerSelection::Pending(selection)) => selection,
            Some(RegisteredWorkerSelection::Retired) => return Ok(()),
            None => {
                return Err(anyhow::anyhow!(
                    "Karva worker {worker_id} connected without a registered selection"
                ));
            }
        }
    };
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

/// Blocking stream read that turns an ignored socket shutdown into EOF.
///
/// The operating-system timeout is hidden from serde so partial JSON frames
/// keep their decoder state. Once interrupted, at most one in-progress read
/// can return bytes before the next call reports EOF.
pub(super) struct InterruptibleReader<'a, R> {
    /// Worker stream consumed by the protocol decoder.
    stream: R,

    /// Controller-owned cancellation flag checked around every system read.
    interrupted: &'a AtomicBool,
}

impl<'a, R> InterruptibleReader<'a, R> {
    /// Wraps one blocking stream with a controller-owned cancellation flag.
    pub(super) fn new(stream: R, interrupted: &'a AtomicBool) -> Self {
        Self {
            stream,
            interrupted,
        }
    }
}

impl<R: Read> Read for InterruptibleReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if self.interrupted.load(Ordering::Acquire) {
                return Ok(0);
            }
            match self.stream.read(buffer) {
                Err(error)
                    if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) =>
                {
                    if self.interrupted.load(Ordering::Acquire) {
                        return Ok(0);
                    }
                }
                result => return result,
            }
        }
    }
}

/// Distinguishes normal socket teardown from a malformed event frame.
pub(super) fn is_clean_disconnect(error: &serde_json::Error) -> bool {
    matches!(
        error.io_error_kind(),
        Some(ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted)
    )
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read};
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::InterruptibleReader;

    struct TimeoutInFrame {
        bytes: &'static [u8],
        position: usize,
        timeout_at: usize,
        timed_out: bool,
    }

    impl Read for TimeoutInFrame {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.position == self.timeout_at && !self.timed_out {
                self.timed_out = true;
                return Err(io::ErrorKind::TimedOut.into());
            }
            if self.position == self.bytes.len() {
                return Ok(0);
            }
            let end = if self.position < self.timeout_at {
                self.timeout_at
            } else {
                self.bytes.len()
            };
            let length = buffer.len().min(end - self.position);
            buffer[..length].copy_from_slice(&self.bytes[self.position..self.position + length]);
            self.position += length;
            Ok(length)
        }
    }

    #[test]
    fn read_timeout_preserves_partial_json_frame() {
        let interrupted = AtomicBool::new(false);
        let stream = TimeoutInFrame {
            bytes: br#"{"event":"complete"}"#,
            position: 0,
            timeout_at: 8,
            timed_out: false,
        };
        let reader = InterruptibleReader::new(stream, &interrupted);

        let value: serde_json::Value =
            serde_json::from_reader(reader).expect("decode frame across timeout");

        assert_eq!(value["event"], "complete");
    }

    struct InterruptOnRead<'a>(&'a AtomicBool);

    impl Read for InterruptOnRead<'_> {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            self.0.store(true, Ordering::Release);
            Err(io::ErrorKind::WouldBlock.into())
        }
    }

    #[test]
    fn interruption_converts_a_blocked_read_to_eof() {
        let interrupted = AtomicBool::new(false);
        let mut reader = InterruptibleReader::new(InterruptOnRead(&interrupted), &interrupted);
        let mut byte = [0_u8];

        assert_eq!(reader.read(&mut byte).expect("interrupt read"), 0);
    }
}
