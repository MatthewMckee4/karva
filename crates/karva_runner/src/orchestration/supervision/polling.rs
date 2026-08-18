//! Process reaping and completion polling for [`super::WorkerSupervisor`].

use std::process::ExitStatus;
use std::time::Instant;

use anyhow::Result;
use crossbeam_channel::{Receiver, TryRecvError};
use karva_ipc::{ControllerServer, WorkerConnectionClose};
use karva_logging::time::format_duration;
use karva_metadata::MaxFail;

use super::super::dispatcher::{CrashCheckpoint, CrashedWorker};
use super::super::output::termination_description;
#[cfg(unix)]
use super::super::process_control;
use super::super::worker::Worker;
use super::super::{CANCELLATION_EVENT_SETTLE, WORKER_POLL_INTERVAL};
use super::WorkerSupervisor;

/// How `wait_for_completion` exited.
#[derive(Debug)]
pub(in crate::orchestration) enum WaitOutcome {
    /// Every worker exited on its own.
    AllCompleted,

    /// Ctrl+C was received; remaining workers must be killed.
    Cancelled,

    /// A worker hit the fail-fast budget; remaining workers must be killed.
    FailFast,

    /// The run timeout elapsed before the workers finished.
    TimedOut,

    /// One or more workers exited unexpectedly.
    WorkersCrashed(Vec<CrashedWorker>),
}

impl WorkerSupervisor {
    fn reap_finished(&mut self, server: &mut ControllerServer) -> Result<Vec<CrashedWorker>> {
        let mut running = Vec::new();
        let mut crashed = Vec::new();
        for mut worker in self.workers.drain(..) {
            let status = if let Some(status) = worker.exit_status() {
                Ok(Some(status))
            } else {
                #[cfg(unix)]
                let status = reap_exited_process_group(&mut worker);
                #[cfg(not(unix))]
                let status = worker.child_mut().try_wait();
                status
            };
            match status {
                Ok(Some(status)) => {
                    if !worker.has_exit_status() {
                        worker.observe_exit(status, server.worker_event_count(worker.id()));
                    }
                    let controller_authenticated = server.worker_started(worker.id())?;
                    let controller_connected =
                        controller_authenticated && !server.worker_disconnected(worker.id());
                    let output_or_events_pending = controller_connected || !worker.output_drained();
                    let mut connection_close = WorkerConnectionClose::Complete;
                    if output_or_events_pending {
                        if controller_connected {
                            let event_count = server.worker_event_count(worker.id());
                            if event_count != worker.event_count() {
                                worker.note_event_count(event_count);
                            }
                        }
                        if !worker.drain_limit_reached() {
                            running.push(worker);
                            continue;
                        }
                        if controller_connected {
                            connection_close = server.close_worker_connection(worker.id())?;
                            self.dispatcher.dispatch_pending(server)?;
                        }
                        worker.mark_forced_disconnect();
                    }
                    let completed =
                        status.success() && self.dispatcher.worker_completed(worker.id());
                    if output_or_events_pending {
                        if completed {
                            tracing::warn!(target: "karva_runner::orchestration",
                                worker_id = worker.id(),
                                limit_ms = CANCELLATION_EVENT_SETTLE.as_millis(),
                                "worker output drain limit reached; final output may be incomplete"
                            );
                        } else {
                            tracing::warn!(target: "karva_runner::orchestration",
                                worker_id = worker.id(),
                                limit_ms = CANCELLATION_EVENT_SETTLE.as_millis(),
                                "worker output drain limit reached; final output and results may be incomplete"
                            );
                        }
                    }
                    worker.join_output();
                    let active = server.take_worker_checkpoint(worker.id())?;
                    if completed {
                        worker.join_stderr(false);
                        tracing::info!(target: "karva_runner::orchestration",
                            "Worker {} completed successfully in {}",
                            worker.id(),
                            format_duration(worker.duration()),
                        );
                    } else {
                        let duration = worker.duration();
                        let stderr = worker.join_stderr(true);
                        tracing::error!(target: "karva_runner::orchestration",
                            "Worker {} failed with {} in {}",
                            worker.id(),
                            termination_description(status),
                            format_duration(duration),
                        );
                        let checkpoint = match connection_close {
                            WorkerConnectionClose::Complete => CrashCheckpoint::Complete(active),
                            WorkerConnectionClose::Forced => CrashCheckpoint::DrainLimited(active),
                        };
                        crashed.push(CrashedWorker {
                            id: worker.id(),
                            partition: worker.take_partition(),
                            status,
                            stderr,
                            checkpoint,
                            controller_authenticated,
                        });
                    }
                }
                Ok(None) => running.push(worker),
                Err(error) => {
                    tracing::error!(target: "karva_runner::orchestration", "Error waiting on worker {}: {}", worker.id(), error);
                }
            }
        }
        self.workers = running;
        Ok(crashed)
    }

    /// Reaps children that stop during termination without classifying their exits.
    pub(in crate::orchestration) fn reap_during_shutdown(&mut self) {
        self.workers
            .retain_mut(|worker| match shutdown_status(worker) {
                Ok(Some(_)) => {
                    worker.mark_forced_disconnect();
                    worker.join_output();
                    worker.join_stderr(false);
                    false
                }
                Ok(None) => true,
                Err(error) => {
                    tracing::error!(target: "karva_runner::orchestration", "Error waiting on worker {}: {}", worker.id(), error);
                    true
                }
            });
    }

    /// Wait for all workers to complete.
    ///
    /// Returns early if a message is received on `shutdown_rx`, the global
    /// failure budget is exhausted, or `deadline` passes. Finished workers are reaped at the
    /// top of each iteration before any of those conditions are checked, so a
    /// run that completes just as the deadline passes (or a signal arrives) is
    /// reported as `AllCompleted` rather than `TimedOut`/`Cancelled`.
    ///
    /// `deadline` is the absolute instant at which the whole run times out; it
    /// is computed before collection so the limit covers the entire run.
    pub(in crate::orchestration) fn wait_for_completion(
        &mut self,
        shutdown_rx: Option<&Receiver<()>>,
        server: &mut ControllerServer,
        max_fail: MaxFail,
        deadline: Option<Instant>,
    ) -> Result<WaitOutcome> {
        if self.workers.is_empty() {
            return Ok(WaitOutcome::AllCompleted);
        }

        tracing::info!(target: "karva_runner::orchestration",
            "Waiting for {} workers to complete (Ctrl+C to cancel)",
            self.workers.len()
        );

        loop {
            self.dispatcher.dispatch_pending(server)?;
            let crashed = self.reap_finished(server)?;
            if !crashed.is_empty() {
                return Ok(WaitOutcome::WorkersCrashed(crashed));
            }

            if self.workers.is_empty() {
                self.dispatcher.finish(server)?;
                let missing = self.dispatcher.missing_workers();
                if !missing.is_empty() {
                    anyhow::bail!("Karva workers {missing:?} exited without sending results");
                }
                tracing::info!(target: "karva_runner::orchestration", "All workers completed");
                return Ok(WaitOutcome::AllCompleted);
            }

            if let Some(rx) = shutdown_rx {
                match rx.try_recv() {
                    Ok(()) | Err(TryRecvError::Disconnected) => {
                        tracing::info!(target: "karva_runner::orchestration", "Shutdown requested — stopping remaining workers");
                        return Ok(WaitOutcome::Cancelled);
                    }
                    Err(TryRecvError::Empty) => {}
                }
            }

            if max_fail.is_exceeded_by(self.dispatcher.failure_count()) {
                tracing::info!(target: "karva_runner::orchestration", "Failure budget exhausted — stopping remaining workers");
                return Ok(WaitOutcome::FailFast);
            }

            if let Some(deadline) = deadline
                && Instant::now() >= deadline
            {
                tracing::info!(target: "karva_runner::orchestration", "Run timeout exceeded — stopping remaining workers");
                return Ok(WaitOutcome::TimedOut);
            }

            std::thread::sleep(WORKER_POLL_INTERVAL);
        }
    }
}

/// Observes one child during graceful shutdown without exposing a recycled
/// Unix process-group id.
///
/// Unix workers remain unreaped until either they are force-killed at the grace
/// deadline or their leader exits. Once a leader exits, any remaining group
/// members are killed before `wait` releases the numeric process-group id.
fn shutdown_status(worker: &mut Worker) -> std::io::Result<Option<ExitStatus>> {
    #[cfg(unix)]
    {
        reap_exited_process_group(worker)
    }
    #[cfg(not(unix))]
    {
        worker.child_mut().try_wait()
    }
}

/// Reaps an exited Unix group leader only after terminating every descendant.
///
/// Keeping the leader waitable reserves its numeric process-group id, so the
/// group signal cannot target an unrelated process after id reuse. This applies
/// to successful workers too: subprocesses created by a test remain owned by
/// that worker generation after the Python runtime reports completion.
#[cfg(unix)]
fn reap_exited_process_group(worker: &mut Worker) -> std::io::Result<Option<ExitStatus>> {
    if !process_control::has_exited(worker.child())? {
        return Ok(None);
    }
    if let Err(error) = process_control::force_kill(worker.child())
        && error.kind() != std::io::ErrorKind::PermissionDenied
    {
        tracing::warn!(target: "karva_runner::orchestration",
            worker_id = worker.id(),
            "failed to clean up worker process group: {error}"
        );
    }
    worker.child_mut().wait().map(Some)
}
