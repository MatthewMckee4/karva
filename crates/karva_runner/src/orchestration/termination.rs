//! Worker termination, cancellation rendering, and final process reaping.

use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::Result;
use colored::Colorize;
use karva_ipc::ControllerServer;
use karva_logging::Printer;

use super::output::{LABEL_COLUMN_WIDTH, format_in_flight_test};
use super::process_control;
use super::supervision::WorkerSupervisor;
use super::worker::Worker;
use super::{CANCELLATION_EVENT_SETTLE, WORKER_POLL_INTERVAL};

/// Snapshot of one worker's current test taken after process termination.
struct InFlightTest {
    /// Worker that owns this snapshot.
    worker_id: usize,

    /// Refined test name, or `None` between tests.
    name: Option<String>,

    /// Elapsed time since test start.
    elapsed: Duration,
}

/// Executing test converted into a synthetic failed result after interruption.
pub(super) struct InterruptedTest {
    /// Test rendered as interrupted after cancellation.
    pub(super) name: String,

    /// Duration measured before worker termination.
    pub(super) duration: Duration,
}

impl WorkerSupervisor {
    /// Terminates all unfinished workers, allowing graceful shutdown first.
    pub(super) fn terminate_remaining(&mut self, grace_period: Duration) {
        if !self.has_workers() {
            return;
        }
        let (workers, dispatcher) = self.worker_state();
        for worker in workers {
            if dispatcher.worker_completed(worker.id()) {
                continue;
            }
            #[cfg(unix)]
            let result = process_control::terminate(worker.child());
            #[cfg(not(unix))]
            let result = process_control::terminate(worker.child_mut());
            if let Err(error) = result {
                tracing::warn!(target: "karva_runner::orchestration",
                    worker_id = worker.id(),
                    "failed to terminate worker process: {error}"
                );
            }
        }

        let deadline = Instant::now() + grace_period;
        loop {
            self.reap_during_shutdown();
            if !self.has_workers() {
                return;
            }
            if grace_period.is_zero() || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(WORKER_POLL_INTERVAL);
        }

        // Signal every retained Unix group before reaping any leader. An
        // unreaped leader reserves its process-group id, so no signal can
        // target a group recycled after an earlier `wait` in this pass.
        // Completed workers can still own test-created descendants.
        #[cfg(unix)]
        for worker in self.workers() {
            if let Err(error) = process_control::force_kill(worker.child())
                && error.kind() != std::io::ErrorKind::PermissionDenied
            {
                tracing::warn!(target: "karva_runner::orchestration",
                    worker_id = worker.id(),
                    "failed to force-kill worker process group: {error}"
                );
            }
        }
        for worker in self.workers_mut() {
            #[cfg(not(unix))]
            if let Err(error) = process_control::force_kill_child(worker.child_mut()) {
                tracing::warn!(target: "karva_runner::orchestration",
                    worker_id = worker.id(),
                    "failed to force-kill worker process: {error}"
                );
            }
            if let Err(error) = worker.child_mut().wait() {
                tracing::warn!(target: "karva_runner::orchestration",
                    worker_id = worker.id(),
                    "failed to wait for worker process: {error}"
                );
            }
            worker.mark_forced_disconnect();
            worker.join_output();
            worker.join_stderr(false);
        }
        self.clear_workers();
    }

    /// Stops workers and renders interruption lines for tests still in flight.
    pub(super) fn cancel_and_kill(
        &mut self,
        printer: Printer,
        server: &mut ControllerServer,
        grace_period: Duration,
    ) -> Result<Vec<InterruptedTest>> {
        if !self.has_workers() {
            return Ok(Vec::new());
        }
        self.dispatch_events(server)?;
        std::thread::sleep(CANCELLATION_EVENT_SETTLE);
        self.dispatch_events(server)?;
        #[expect(
            clippy::needless_collect,
            reason = "termination clears workers before late events are attributed"
        )]
        let worker_ids: Vec<_> = self.workers().iter().map(Worker::id).collect();
        self.terminate_remaining(grace_period);
        server.disconnect_readers()?;
        self.finish_events(server)?;

        let in_flight = worker_ids
            .into_iter()
            .map(|worker_id| {
                Ok(match server.take_worker_checkpoint(worker_id)? {
                    Some(current) => InFlightTest {
                        worker_id,
                        name: Some(current.name),
                        elapsed: current.started.elapsed(),
                    },
                    None => InFlightTest {
                        worker_id,
                        name: None,
                        elapsed: Duration::ZERO,
                    },
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let running_tests = in_flight.iter().filter(|test| test.name.is_some()).count();
        let test_label = if running_tests == 1 { "test" } else { "tests" };
        let mut stdout = printer.stream_for_test_result().lock();
        let cancel_label = "Cancelling".yellow().bold();
        let interrupt_label = "interrupt".yellow().bold();
        if let Err(error) = writeln!(
            stdout,
            "  {cancel_label} due to {interrupt_label}: {running_tests} {test_label} still running"
        ) {
            tracing::warn!(target: "karva_runner::orchestration", "failed to write cancellation banner: {error}");
        }

        let label = "SIGINT".yellow().bold();
        let padding = " ".repeat(LABEL_COLUMN_WIDTH.saturating_sub("SIGINT".len()));
        for test in &in_flight {
            let duration = karva_logging::time::format_duration_bracketed(test.elapsed);
            match &test.name {
                Some(name) => {
                    if let Err(error) = writeln!(
                        stdout,
                        "{padding}{label} {duration} {}",
                        format_in_flight_test(name)
                    ) {
                        tracing::warn!(target: "karva_runner::orchestration", "failed to write interrupted test line: {error}");
                    }
                }
                None => {
                    if let Err(error) = writeln!(
                        stdout,
                        "{padding}{label} {duration} worker {} (between tests)",
                        test.worker_id
                    ) {
                        tracing::warn!(target: "karva_runner::orchestration", "failed to write interrupted worker line: {error}");
                    }
                }
            }
        }

        Ok(in_flight
            .into_iter()
            .filter_map(|test| {
                test.name.map(|name| InterruptedTest {
                    name,
                    duration: test.elapsed,
                })
            })
            .collect())
    }
}

impl Drop for WorkerSupervisor {
    fn drop(&mut self) {
        for worker in self.workers_mut() {
            if !worker.has_exit_status()
                && let Err(error) = process_control::force_kill(worker.child())
            {
                tracing::warn!(target: "karva_runner::orchestration",
                    worker_id = worker.id(),
                    "failed to clean up worker process group: {error}"
                );
            }
            #[cfg(not(unix))]
            if let Err(error) = process_control::force_kill_child(worker.child_mut()) {
                tracing::warn!(target: "karva_runner::orchestration", worker_id = worker.id(), "failed to kill worker: {error}");
            }
            if let Err(error) = worker.child_mut().wait() {
                tracing::warn!(target: "karva_runner::orchestration", worker_id = worker.id(), "failed to reap worker: {error}");
            }
        }
    }
}
