//! Worker output draining and bounded crash-diagnostic capture.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStderr, ChildStdout};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

// Receipt: the Python 3.14 abort diagnostic observed in CI was 5,848 bytes.
// This preserves more than 179 such diagnostics without letting noisy tests
// grow the controller's per-worker spool without bound.
const MAX_WORKER_STDERR_CAPTURE_BYTES: usize = 1024 * 1024;

/// Background stdout drain that preserves worker output ordering.
#[derive(Debug)]
pub(super) struct WorkerOutputForwarder {
    /// Thread draining worker stdout in order.
    handle: JoinHandle<std::io::Result<()>>,

    /// Shared stop flag used after forced disconnect.
    enabled: Arc<AtomicBool>,
}

/// Background stderr drain that forwards output and keeps a bounded copy.
#[derive(Debug)]
pub(super) struct WorkerStderrForwarder {
    /// Thread forwarding and capturing worker stderr.
    handle: JoinHandle<std::io::Result<()>>,

    /// Shared stop flag used after forced disconnect.
    enabled: Arc<AtomicBool>,
}

impl WorkerStderrForwarder {
    pub(super) fn spawn(stderr: ChildStderr, captured: File) -> Self {
        let enabled = Arc::new(AtomicBool::new(true));
        let forwarder_enabled = Arc::clone(&enabled);
        let handle =
            thread::spawn(move || forward_worker_stderr(stderr, captured, &forwarder_enabled));
        Self { handle, enabled }
    }

    pub(super) fn join(self, worker_id: usize, wait: bool) {
        if !wait && !self.handle.is_finished() {
            stop_forwarding(&self.enabled);
            return;
        }
        match self.handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::warn!(target: "karva_runner::orchestration", worker_id, "failed to forward worker stderr: {error}");
            }
            Err(error) => {
                tracing::warn!(target: "karva_runner::orchestration", worker_id, ?error, "worker stderr forwarder panicked");
            }
        }
    }

    /// Whether the worker stderr pipe reached EOF and forwarding stopped.
    pub(super) fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }
}

impl WorkerOutputForwarder {
    pub(super) fn spawn(stdout: ChildStdout) -> Self {
        let enabled = Arc::new(AtomicBool::new(true));
        let forwarder_enabled = Arc::clone(&enabled);
        let handle = thread::spawn(move || forward_worker_stdout(stdout, &forwarder_enabled));
        Self { handle, enabled }
    }

    pub(super) fn join(self, worker_id: usize, wait: bool) {
        if !wait && !self.handle.is_finished() {
            stop_forwarding(&self.enabled);
            return;
        }
        match self.handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(err)) if err.kind() == std::io::ErrorKind::BrokenPipe => {}
            Ok(Err(err)) => {
                tracing::warn!(target: "karva_runner::orchestration", worker_id, "failed to forward worker stdout: {err}");
            }
            Err(err) => {
                tracing::warn!(target: "karva_runner::orchestration", worker_id, ?err, "worker stdout forwarder panicked");
            }
        }
    }

    /// Whether the worker stdout pipe reached EOF and forwarding stopped.
    pub(super) fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }
}

/// Stops future forwarding without waiting for an in-progress output write.
fn stop_forwarding(enabled: &AtomicBool) {
    enabled.store(false, Ordering::Relaxed);
}

fn forward_worker_stdout(stdout: ChildStdout, enabled: &AtomicBool) -> std::io::Result<()> {
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();

    loop {
        line.clear();
        let bytes_read = reader.read_until(b'\n', &mut line)?;
        if bytes_read == 0 {
            return Ok(());
        }

        // Batch complete lines already buffered from the pipe. Retaining a
        // partial final line prevents output from different workers interleaving.
        loop {
            let complete_bytes = {
                let buffered = reader.fill_buf()?;
                let Some(last_newline) = buffered.iter().rposition(|byte| *byte == b'\n') else {
                    break;
                };
                line.extend_from_slice(&buffered[..=last_newline]);
                last_newline + 1
            };
            reader.consume(complete_bytes);
        }

        if !enabled.load(Ordering::Relaxed) {
            return Ok(());
        }
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&line)?;
    }
}

fn forward_worker_stderr(
    stderr: ChildStderr,
    mut captured: File,
    enabled: &AtomicBool,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stderr);
    let mut capture_error = None;
    let mut forward_error = None;
    let mut last_byte = None;
    let mut received_bytes = 0_usize;
    let mut captured_bytes = 0_usize;
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            break;
        }
        let consumed = bytes.len();
        if !enabled.load(Ordering::Relaxed) {
            return Ok(());
        }
        received_bytes = received_bytes.saturating_add(consumed);
        last_byte = bytes.last().copied();
        if capture_error.is_none() && captured_bytes < MAX_WORKER_STDERR_CAPTURE_BYTES {
            let remaining = MAX_WORKER_STDERR_CAPTURE_BYTES - captured_bytes;
            let capture = &bytes[..bytes.len().min(remaining)];
            if let Err(error) = captured.write_all(capture) {
                capture_error = Some(error);
            } else {
                captured_bytes += capture.len();
            }
        }
        if forward_error.is_none()
            && let Err(error) = std::io::stderr().lock().write_all(bytes)
        {
            forward_error = Some(error);
        }
        reader.consume(consumed);
    }
    if last_byte.is_some_and(|byte| byte != b'\n')
        && forward_error.is_none()
        && let Err(error) = std::io::stderr().lock().write_all(b"\n")
    {
        forward_error = Some(error);
    }
    if received_bytes > MAX_WORKER_STDERR_CAPTURE_BYTES && capture_error.is_none() {
        let marker = format!(
            "\n[Karva worker stderr capture exceeded its {MAX_WORKER_STDERR_CAPTURE_BYTES}-byte limit; received {received_bytes} bytes]\n"
        );
        if let Err(error) = captured.write_all(marker.as_bytes()) {
            capture_error = Some(error);
        }
    }
    if capture_error.is_none()
        && let Err(error) = captured.flush()
    {
        capture_error = Some(error);
    }
    if let Some(error) = capture_error {
        Err(error)
    } else if let Some(error) = forward_error {
        if error.kind() == std::io::ErrorKind::BrokenPipe {
            Ok(())
        } else {
            Err(error)
        }
    } else {
        Ok(())
    }
}
