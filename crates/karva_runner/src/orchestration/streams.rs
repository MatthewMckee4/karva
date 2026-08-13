//! Worker output draining and bounded crash-diagnostic capture.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStderr, ChildStdout};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

// Receipt: the Python 3.14 abort diagnostic observed in CI was 5,848 bytes.
// This preserves more than 179 such diagnostics without letting noisy tests
// grow the controller's per-worker spool without bound.
const MAX_WORKER_STDERR_CAPTURE_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
/// Background stdout drain that preserves worker output ordering.
pub(super) struct WorkerOutputForwarder {
    /// Thread draining worker stdout in order.
    handle: JoinHandle<std::io::Result<()>>,
    /// Shared stop flag used after forced disconnect.
    enabled: Arc<Mutex<bool>>,
}

#[derive(Debug)]
/// Background stderr drain that forwards output and keeps a bounded copy.
pub(super) struct WorkerStderrForwarder {
    /// Thread forwarding and capturing worker stderr.
    handle: JoinHandle<std::io::Result<()>>,
    /// Shared stop flag used after forced disconnect.
    enabled: Arc<Mutex<bool>>,
}

impl WorkerStderrForwarder {
    pub(super) fn spawn(stderr: ChildStderr, captured: File) -> Self {
        let enabled = Arc::new(Mutex::new(true));
        let forwarder_enabled = Arc::clone(&enabled);
        let handle =
            thread::spawn(move || forward_worker_stderr(stderr, captured, &forwarder_enabled));
        Self { handle, enabled }
    }

    pub(super) fn join(self, worker_id: usize, wait: bool) {
        if !wait && !self.handle.is_finished() {
            stop_forwarding(&self.enabled, worker_id, "stderr");
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
}

impl WorkerOutputForwarder {
    pub(super) fn spawn(stdout: ChildStdout) -> Self {
        let enabled = Arc::new(Mutex::new(true));
        let forwarder_enabled = Arc::clone(&enabled);
        let handle = thread::spawn(move || forward_worker_stdout(stdout, &forwarder_enabled));
        Self { handle, enabled }
    }

    pub(super) fn join(self, worker_id: usize, wait: bool) {
        if !wait && !self.handle.is_finished() {
            stop_forwarding(&self.enabled, worker_id, "stdout");
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
}

fn stop_forwarding(enabled: &Mutex<bool>, worker_id: usize, stream: &str) {
    match enabled.lock() {
        Ok(mut enabled) => *enabled = false,
        Err(error) => {
            tracing::warn!(target: "karva_runner::orchestration", worker_id, "failed to stop worker {stream}: {error}");
        }
    }
}

fn forward_worker_stdout(stdout: ChildStdout, enabled: &Mutex<bool>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();

    loop {
        line.clear();
        let bytes_read = reader.read_until(b'\n', &mut line)?;
        if bytes_read == 0 {
            return Ok(());
        }

        let enabled = enabled.lock().map_err(|error| {
            std::io::Error::other(format!("stdout forwarding lock poisoned: {error}"))
        })?;
        if !*enabled {
            return Ok(());
        }
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&line)?;
        drop(enabled);
    }
}

fn forward_worker_stderr(
    stderr: ChildStderr,
    mut captured: File,
    enabled: &Mutex<bool>,
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
        let enabled = enabled.lock().map_err(|error| {
            std::io::Error::other(format!("stderr forwarding lock poisoned: {error}"))
        })?;
        if !*enabled {
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
        drop(enabled);
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
