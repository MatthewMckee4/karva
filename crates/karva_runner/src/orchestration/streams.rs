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

use karva_ipc::output_capture::{OutputCaptureSink, OutputStream};

/// Background stdout drain that preserves worker output ordering.
#[derive(Debug)]
pub(super) struct WorkerOutputForwarder {
    /// Thread draining worker stdout in order.
    handle: JoinHandle<std::io::Result<()>>,

    /// Shared stop flag used after forced disconnect.
    enabled: Arc<AtomicBool>,

    /// Native output windows shared with the authenticated worker connection.
    capture: Arc<OutputCaptureSink>,
}

/// Background stderr drain that forwards output and keeps a bounded copy.
#[derive(Debug)]
pub(super) struct WorkerStderrForwarder {
    /// Thread forwarding and capturing worker stderr.
    handle: JoinHandle<std::io::Result<()>>,

    /// Shared stop flag used after forced disconnect.
    enabled: Arc<AtomicBool>,

    /// Native output windows shared with the authenticated worker connection.
    capture: Arc<OutputCaptureSink>,
}

impl WorkerStderrForwarder {
    pub(super) fn spawn(
        stderr: ChildStderr,
        captured: File,
        capture: Arc<OutputCaptureSink>,
    ) -> Self {
        let enabled = Arc::new(AtomicBool::new(true));
        let forwarder_enabled = Arc::clone(&enabled);
        let reader_capture = Arc::clone(&capture);
        let handle = thread::spawn(move || {
            let result =
                forward_worker_stderr(stderr, captured, &forwarder_enabled, &reader_capture);
            let _ = reader_capture.close(OutputStream::Stderr);
            result
        });
        Self {
            handle,
            enabled,
            capture,
        }
    }

    pub(super) fn join(self, worker_id: usize, wait: bool) {
        if !wait && !self.handle.is_finished() {
            stop_forwarding(&self.enabled);
            let _ = self.capture.close(OutputStream::Stderr);
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
    pub(super) fn spawn(
        stdout: ChildStdout,
        capture: Arc<OutputCaptureSink>,
        forward: bool,
    ) -> Self {
        let enabled = Arc::new(AtomicBool::new(true));
        let forwarder_enabled = Arc::clone(&enabled);
        let reader_capture = Arc::clone(&capture);
        let handle = thread::spawn(move || {
            let result =
                forward_worker_stdout(stdout, &forwarder_enabled, &reader_capture, forward);
            let _ = reader_capture.close(OutputStream::Stdout);
            result
        });
        Self {
            handle,
            enabled,
            capture,
        }
    }

    pub(super) fn join(self, worker_id: usize, wait: bool) {
        if !wait && !self.handle.is_finished() {
            stop_forwarding(&self.enabled);
            let _ = self.capture.close(OutputStream::Stdout);
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

fn forward_worker_stdout(
    stdout: ChildStdout,
    enabled: &AtomicBool,
    capture: &OutputCaptureSink,
    forward: bool,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            line.extend(capture.close(OutputStream::Stdout)?);
            if forward && enabled.load(Ordering::Relaxed) {
                std::io::stdout().lock().write_all(&line)?;
            }
            return Ok(());
        }
        let count = bytes.len();
        if !enabled.load(Ordering::Relaxed) {
            return Ok(());
        }
        let outside = capture.push(OutputStream::Stdout, bytes)?;
        if forward {
            line.extend(outside);
        }
        if let Some(end) = line.iter().rposition(|byte| *byte == b'\n') {
            std::io::stdout().lock().write_all(&line[..=end])?;
            line.drain(..=end);
        }
        reader.consume(count);
        if capture.is_closed(OutputStream::Stdout) {
            if forward {
                std::io::stdout().lock().write_all(&line)?;
            }
            return Ok(());
        }
    }
}

fn forward_worker_stderr(
    stderr: ChildStderr,
    mut captured: File,
    enabled: &AtomicBool,
    capture: &OutputCaptureSink,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stderr);
    let mut capture_error = None;
    let mut forward_error = None;
    let mut last_byte = None;
    let mut received_bytes = 0_usize;
    let mut captured_bytes = 0_usize;
    loop {
        let raw = reader.fill_buf()?;
        let consumed = raw.len();
        let bytes = if consumed == 0 {
            capture.close(OutputStream::Stderr)?
        } else {
            capture.push(OutputStream::Stderr, raw)?
        };
        if consumed == 0 && bytes.is_empty() {
            break;
        }
        if !enabled.load(Ordering::Relaxed) {
            return Ok(());
        }
        received_bytes = received_bytes.saturating_add(bytes.len());
        if !bytes.is_empty() {
            last_byte = bytes.last().copied();
        }
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
            && let Err(error) = std::io::stderr().lock().write_all(&bytes)
        {
            forward_error = Some(error);
        }
        reader.consume(consumed);
        if consumed == 0 || capture.is_closed(OutputStream::Stderr) {
            break;
        }
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
