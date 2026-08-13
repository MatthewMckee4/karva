//! Worker polling, event draining, and unexpected-exit detection.

use std::time::{Duration, Instant};

use anyhow::Result;
use crossbeam_channel::{Receiver, TryRecvError};
use karva_ipc::ControllerServer;
use karva_logging::time::format_duration;
use karva_metadata::MaxFail;

use super::config::TestResultRetention;
use super::dispatcher::{CrashedWorker, EventDispatcher};
use super::output::termination_description;
#[cfg(unix)]
use super::process_control;
use super::worker::Worker;
use super::{CANCELLATION_EVENT_SETTLE, WORKER_POLL_INTERVAL};
use crate::partition::Partition;

/// How `wait_for_completion` exited.
#[derive(Debug)]
pub(super) enum WaitOutcome {
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

/// Owns live workers and serializes their process and event lifecycle.
pub(super) struct WorkerSupervisor {
    /// Child generations that have not completed process and stream cleanup.
    ///
    /// Module visibility lets the separate termination implementation perform
    /// process cleanup without exposing this state outside orchestration.
    pub(super) workers: Vec<Worker>,

    /// Serialized event state shared with crash recovery and cancellation.
    ///
    /// Termination drains this state after processes stop so late events retain
    /// their original worker attribution.
    pub(super) dispatcher: EventDispatcher,
}

impl WorkerSupervisor {
    pub(super) fn has_workers(&self) -> bool {
        !self.workers.is_empty()
    }

    pub(super) fn worker_completed(&self, worker_id: usize) -> bool {
        self.dispatcher.worker_completed(worker_id)
    }

    pub(super) fn completed_test_keys(
        &self,
    ) -> std::collections::HashSet<karva_python_semantic::TestCacheKey> {
        self.dispatcher.completed_test_keys()
    }

    pub(super) fn abandon_worker(&mut self, worker_id: usize) {
        self.dispatcher.abandon_worker(worker_id);
    }

    pub(super) fn register_worker_exit(
        &mut self,
        worker_id: usize,
        termination: &str,
        stderr: &str,
    ) {
        self.dispatcher
            .register_worker_exit(worker_id, termination, stderr);
    }

    pub(super) fn register_crashed_test(
        &mut self,
        name: &str,
        cache_key: karva_python_semantic::TestCacheKey,
        duration: Duration,
        termination: &str,
        stderr: &str,
    ) {
        self.dispatcher
            .register_crashed_test(name, cache_key, duration, termination, stderr);
    }

    pub(super) fn failure_count(&self) -> u32 {
        self.dispatcher.failure_count()
    }

    pub(super) fn finish_events(&mut self, server: &mut ControllerServer) -> Result<()> {
        self.dispatcher.finish(server)
    }

    pub(super) fn dispatch_events(&mut self, server: &mut ControllerServer) -> Result<()> {
        self.dispatcher.dispatch_pending(server)
    }

    pub(super) fn take_results(&mut self) -> karva_diagnostic::AggregatedResults {
        self.dispatcher.take_results()
    }

    pub(super) fn with_test_capacity(
        test_capacity: usize,
        result_retention: TestResultRetention,
    ) -> Self {
        Self {
            workers: Vec::new(),
            dispatcher: EventDispatcher::with_test_capacity(test_capacity, result_retention),
        }
    }
}

impl WorkerSupervisor {
    pub(super) fn spawn(
        &mut self,
        worker_id: usize,
        partition: Partition,
        resources: super::worker::WorkerResources,
    ) {
        self.dispatcher.register_worker(worker_id);
        self.workers
            .push(Worker::new(worker_id, partition, resources));
    }

    fn reap_finished(&mut self, server: &ControllerServer) -> Result<Vec<CrashedWorker>> {
        let mut running = Vec::new();
        let mut crashed = Vec::new();
        for mut worker in self.workers.drain(..) {
            let status = if let Some(status) = worker.exit_status() {
                Ok(Some(status))
            } else {
                #[cfg(unix)]
                if process_control::has_exited(worker.child())? {
                    if !server.worker_disconnected(worker.id())
                        && let Err(error) = process_control::force_kill(worker.process_id())
                        && error.kind() != std::io::ErrorKind::PermissionDenied
                    {
                        tracing::warn!(target: "karva_runner::orchestration",
                            worker_id = worker.id(),
                            "failed to clean up worker process group: {error}"
                        );
                    }
                    worker.child_mut().wait().map(Some)
                } else {
                    Ok(None)
                }
                #[cfg(not(unix))]
                worker.child_mut().try_wait()
            };
            match status {
                Ok(Some(status)) => {
                    if !worker.has_exit_status() {
                        worker.observe_exit(status, server.worker_event_count(worker.id()));
                    }
                    if server.worker_started(worker.id())?
                        && !server.worker_disconnected(worker.id())
                    {
                        let event_count = server.worker_event_count(worker.id());
                        if event_count != worker.event_count() {
                            worker.note_event_count(event_count);
                        }
                        if worker.drain_limit_reached() {
                            server.disconnect_worker(worker.id())?;
                            worker.mark_forced_disconnect();
                            tracing::warn!(target: "karva_runner::orchestration",
                                worker_id = worker.id(),
                                limit_ms = CANCELLATION_EVENT_SETTLE.as_millis(),
                                "worker output drain limit reached; final output and results may be incomplete"
                            );
                        }
                        running.push(worker);
                        continue;
                    }
                    worker.join_output();
                    let completed =
                        status.success() && self.dispatcher.worker_completed(worker.id());
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
                        let active = self.dispatcher.take_active_test(worker.id());
                        crashed.push(CrashedWorker {
                            id: worker.id(),
                            partition: worker.take_partition(),
                            status,
                            stderr,
                            active,
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

    pub(super) fn reap_during_shutdown(&mut self) {
        self.workers
            .retain_mut(|worker| match worker.child_mut().try_wait() {
                Ok(Some(_)) => {
                    worker.join_output();
                    worker.join_stderr(false);
                    false
                }
                Ok(None) => true,
                Err(error) => {
                    tracing::error!(target: "karva_runner::orchestration", "Error waiting on worker {}: {}", worker.id(), error);
                    false
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
    pub(super) fn wait_for_completion(
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
