//! State owned for one worker generation.
//!
//! Assignment, process, stream, and lifecycle state stay grouped by purpose;
//! supervision can follow transitions without a flat collection of unrelated
//! fields.

use std::io::{Read, Seek};
use std::process::{Child, ExitStatus};
use std::time::{Duration, Instant};

use tempfile::NamedTempFile;

use crate::partition::Partition;

use super::CANCELLATION_EVENT_SETTLE;
use super::streams::{WorkerOutputForwarder, WorkerStderrForwarder};

/// One worker's test assignment, process, streams, and lifecycle state.
#[derive(Debug)]
pub(super) struct Worker {
    /// Stable controller identifier used in IPC messages.
    assignment: WorkerAssignment,

    /// Child process owned and reaped by the supervisor.
    process: WorkerProcess,

    /// Forwarded output and bounded crash-diagnostic capture.
    streams: WorkerStreams,

    /// Exit observation and event-drain state.
    lifecycle: WorkerLifecycle,
}

/// Resources acquired while constructing one child process.
pub(super) struct WorkerResources {
    /// Spawned child process.
    pub(super) child: Child,

    /// Optional stdout forwarder.
    pub(super) output: Option<WorkerOutputForwarder>,

    /// Stderr forwarder.
    pub(super) stderr: WorkerStderrForwarder,

    /// Temporary bounded stderr capture.
    pub(super) stderr_capture: NamedTempFile,
}

/// Tests owned by one worker generation.
#[derive(Debug)]
struct WorkerAssignment {
    /// Controller-assigned worker identifier.
    id: usize,

    /// Tests to retry if this worker crashes.
    partition: Partition,
}

/// Child process and start timestamp.
#[derive(Debug)]
struct WorkerProcess {
    /// Spawned karva-worker process.
    child: Child,

    /// Timestamp used for worker and crash durations.
    started_at: Instant,
}

/// Worker stdout/stderr forwarding and captured stderr spool.
#[derive(Debug)]
struct WorkerStreams {
    /// Optional controller-forwarded stdout.
    output: Option<WorkerOutputForwarder>,

    /// Background stderr forwarder.
    stderr: Option<WorkerStderrForwarder>,

    /// Bounded stderr retained for crash diagnostics.
    stderr_capture: NamedTempFile,
}

/// Mutable state after a worker exits.
#[derive(Debug, Default)]
struct WorkerLifecycle {
    /// Exit status observed without reaping, then confirmed by wait.
    exit_status: Option<ExitStatus>,

    /// Time when exit was first observed.
    exit_observed: Option<Instant>,

    /// Number of IPC events observed at exit.
    exit_event_count: usize,

    /// Whether the controller stopped waiting for late output/events.
    forced_disconnect: bool,
}

impl Worker {
    pub(super) fn id(&self) -> usize {
        self.assignment.id
    }

    pub(super) fn process_id(&self) -> u32 {
        self.process.child.id()
    }

    pub(super) fn take_partition(self) -> Partition {
        self.assignment.partition
    }

    #[cfg(unix)]
    pub(super) fn child(&self) -> &Child {
        &self.process.child
    }

    pub(super) fn child_mut(&mut self) -> &mut Child {
        &mut self.process.child
    }

    pub(super) fn has_exit_status(&self) -> bool {
        self.lifecycle.exit_status.is_some()
    }

    pub(super) fn exit_status(&self) -> Option<ExitStatus> {
        self.lifecycle.exit_status
    }

    /// Records the first observed process exit and the IPC progress at that instant.
    pub(super) fn observe_exit(&mut self, status: ExitStatus, event_count: usize) {
        self.lifecycle.exit_status = Some(status);
        self.lifecycle.exit_observed = Some(Instant::now());
        self.lifecycle.exit_event_count = event_count;
    }

    pub(super) fn event_count(&self) -> usize {
        self.lifecycle.exit_event_count
    }

    /// Extends the drain window after late IPC progress from an exited worker.
    pub(super) fn note_event_count(&mut self, event_count: usize) {
        self.lifecycle.exit_event_count = event_count;
        self.lifecycle.exit_observed = Some(Instant::now());
    }

    /// Returns whether an exited worker has made no IPC progress for one drain window.
    pub(super) fn drain_limit_reached(&self) -> bool {
        self.lifecycle
            .exit_observed
            .is_some_and(|observed| observed.elapsed() >= CANCELLATION_EVENT_SETTLE)
    }

    /// Marks output forwarders as detached so cleanup never blocks on inherited handles.
    pub(super) fn mark_forced_disconnect(&mut self) {
        self.lifecycle.forced_disconnect = true;
    }

    pub(super) fn new(id: usize, partition: Partition, resources: WorkerResources) -> Self {
        Self {
            assignment: WorkerAssignment { id, partition },
            process: WorkerProcess {
                child: resources.child,
                started_at: Instant::now(),
            },
            streams: WorkerStreams {
                output: resources.output,
                stderr: Some(resources.stderr),
                stderr_capture: resources.stderr_capture,
            },
            lifecycle: WorkerLifecycle::default(),
        }
    }

    pub(super) fn duration(&self) -> Duration {
        self.process.started_at.elapsed()
    }

    pub(super) fn join_output(&mut self) {
        if let Some(output) = self.streams.output.take() {
            output.join(self.assignment.id, !self.lifecycle.forced_disconnect);
        }
    }

    pub(super) fn join_stderr(&mut self, read: bool) -> String {
        if let Some(stderr) = self.streams.stderr.take() {
            stderr.join(self.assignment.id, !self.lifecycle.forced_disconnect);
        }
        if !read {
            return String::new();
        }
        let file = self.streams.stderr_capture.as_file_mut();
        let output = match file.rewind().and_then(|()| {
            let mut output = Vec::new();
            file.read_to_end(&mut output)?;
            Ok(output)
        }) {
            Ok(output) => String::from_utf8_lossy(&output).into_owned(),
            Err(error) => {
                tracing::warn!(target: "karva_runner::orchestration",
                    worker_id = self.assignment.id,
                    "failed to read worker stderr: {error}"
                );
                String::new()
            }
        };
        if self.lifecycle.forced_disconnect {
            let separator = if output.is_empty() || output.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            format!(
                "{output}{separator}[Karva stopped draining worker output after the {} ms limit; final output and results may be incomplete]\n",
                CANCELLATION_EVENT_SETTLE.as_millis()
            )
        } else {
            output
        }
    }
}
