//! Event ordering and controller-owned test state.
//!
//! Reader threads only decode complete IPC frames. This module applies those
//! frames serially so result aggregation and active-test attribution have one
//! owner.

use std::collections::{HashMap, HashSet};
use std::process::ExitStatus;
use std::time::{Duration, Instant};

use anyhow::Result;
use karva_diagnostic::AggregatedResults;
use karva_ipc::{ControllerServer, WorkerEvent};
use karva_python_semantic::TestCacheKey;

use super::config::TestResultRetention;
use crate::partition::Partition;

/// Linearizes worker events into controller-owned run state.
#[derive(Default)]
pub(super) struct EventDispatcher {
    /// Worker generations allowed to send events for the current run.
    expected_workers: HashSet<usize>,

    /// Worker generations that sent their terminal lifecycle event.
    completed_workers: HashSet<usize>,

    /// Latest started but unfinished test for each worker generation.
    in_flight: HashMap<usize, RunningTest>,

    /// Results and diagnostics aggregated across every worker generation.
    results: AggregatedResults,

    /// Completed case bodies retained for final report formats.
    result_retention: TestResultRetention,
}

/// Controller-owned state for one executing test.
#[derive(Debug)]
pub(super) struct RunningTest {
    /// Refined display name, including fixture-derived parameters.
    pub(super) name: String,
    /// Cache identity used to match completion and crash recovery.
    pub(super) cache_key: TestCacheKey,
    /// Monotonic start time used for interruption duration.
    pub(super) started: Instant,
}

#[derive(Debug)]
/// Unexpected worker exit plus state needed to report and retry it.
pub(super) struct CrashedWorker {
    /// Worker generation that exited unexpectedly.
    pub(super) id: usize,
    /// Remaining selection eligible for replacement execution.
    pub(super) partition: Partition,
    /// Exit status captured before process reaping.
    pub(super) status: ExitStatus,
    /// Bounded stderr diagnostic captured from the worker.
    pub(super) stderr: String,
    /// Test active when process exited, if any.
    pub(super) active: Option<RunningTest>,
}

/// Snapshot of one worker's current test taken before process termination.
pub(super) struct InFlightTest {
    /// Worker that owns this snapshot.
    pub(super) worker_id: usize,
    /// Refined test name, or `None` between tests.
    pub(super) name: Option<String>,
    /// Elapsed time since test start.
    pub(super) elapsed: Duration,
}

/// Executing test converted into a synthetic failed result after interruption.
pub(super) struct InterruptedTest {
    /// Test rendered as interrupted after cancellation.
    pub(super) name: String,
    /// Duration measured before worker termination.
    pub(super) duration: Duration,
}

impl EventDispatcher {
    /// Allocates run aggregation with capacity matching its retention policy.
    pub(super) fn with_test_capacity(
        test_capacity: usize,
        result_retention: TestResultRetention,
    ) -> Self {
        let test_case_capacity = match result_retention {
            TestResultRetention::FailuresAndRetries => 0,
            TestResultRetention::All => test_capacity,
        };
        Self {
            expected_workers: HashSet::new(),
            completed_workers: HashSet::new(),
            in_flight: HashMap::new(),
            results: AggregatedResults::with_capacities(test_capacity, test_case_capacity),
            result_retention,
        }
    }

    /// Admits one worker generation before its process can send events.
    pub(super) fn register_worker(&mut self, worker_id: usize) {
        self.expected_workers.insert(worker_id);
    }

    /// Applies every queued worker event to controller-owned run state.
    pub(super) fn dispatch_pending(&mut self, server: &mut ControllerServer) -> Result<()> {
        server.accept_pending()?;
        while let Some(message) = server.try_recv()? {
            let worker_id = message.worker_id;
            if !self.expected_workers.contains(&worker_id) {
                anyhow::bail!("unknown Karva worker {worker_id} sent a controller event");
            }
            match *message.event {
                WorkerEvent::TestStarted { name, cache_key } => {
                    if let Some(running) = self.in_flight.get_mut(&worker_id) {
                        if running.cache_key.test_function_name() != cache_key.test_function_name()
                        {
                            anyhow::bail!(
                                "Karva worker {worker_id} started `{name}` before finishing `{}`",
                                running.name
                            );
                        }
                        running.name = name;
                        running.cache_key = cache_key;
                    } else {
                        self.in_flight.insert(
                            worker_id,
                            RunningTest {
                                name,
                                cache_key,
                                started: Instant::now(),
                            },
                        );
                    }
                }
                WorkerEvent::TestSlow => self.results.register_slow_test(),
                WorkerEvent::TestFinished { cache_key, result } => {
                    if let Some(running) = self.in_flight.remove(&worker_id)
                        && running.cache_key != cache_key
                    {
                        anyhow::bail!(
                            "Karva worker {worker_id} started `{}` but finished `{}`",
                            running.name,
                            result.full_name()
                        );
                    }
                    self.results.register_rendered_test_case(
                        cache_key,
                        *result,
                        matches!(self.result_retention, TestResultRetention::All),
                    );
                }
                WorkerEvent::RunDiagnostic(diagnostic) => {
                    self.results.add_rendered_run_diagnostic(diagnostic);
                }
                WorkerEvent::WorkerFinished => {
                    if !self.completed_workers.insert(worker_id) {
                        anyhow::bail!("Karva worker {worker_id} completed more than once");
                    }
                    if let Some(running) = self.in_flight.get(&worker_id) {
                        anyhow::bail!(
                            "Karva worker {worker_id} completed while `{}` was still running",
                            running.name
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Joins IPC readers, then applies every event they delivered before EOF.
    pub(super) fn finish(&mut self, server: &mut ControllerServer) -> Result<()> {
        server.finish()?;
        self.dispatch_pending(server)?;
        Ok(())
    }

    /// Removes a crashed generation so its replacement owns future events.
    pub(super) fn abandon_worker(&mut self, worker_id: usize) {
        self.expected_workers.remove(&worker_id);
        self.completed_workers.remove(&worker_id);
    }

    /// Builds crash-recovery membership only when recovery needs it.
    pub(super) fn completed_test_keys(&self) -> HashSet<TestCacheKey> {
        self.results.durations.keys().cloned().collect()
    }

    pub(super) fn worker_completed(&self, worker_id: usize) -> bool {
        self.completed_workers.contains(&worker_id)
    }

    pub(super) fn missing_workers(&self) -> Vec<usize> {
        let mut missing = self
            .expected_workers
            .difference(&self.completed_workers)
            .copied()
            .collect::<Vec<_>>();
        missing.sort_unstable();
        missing
    }

    pub(super) fn register_worker_exit(
        &mut self,
        worker_id: usize,
        termination: &str,
        stderr: &str,
    ) {
        self.results
            .register_worker_exit(worker_id, termination, stderr);
    }

    pub(super) fn register_crashed_test(
        &mut self,
        name: &str,
        cache_key: TestCacheKey,
        duration: Duration,
        termination: &str,
        stderr: &str,
    ) {
        self.results
            .register_crashed_test(name, cache_key, duration, termination, stderr);
    }

    pub(super) fn failure_count(&self) -> u32 {
        let failures = self.results.stats().failed() + self.results.stats().errors();
        u32::try_from(failures).unwrap_or(u32::MAX)
    }

    pub(super) fn take_results(&mut self) -> AggregatedResults {
        std::mem::take(&mut self.results)
    }

    pub(super) fn active_test(&self, worker_id: usize) -> Option<&RunningTest> {
        self.in_flight.get(&worker_id)
    }

    pub(super) fn take_active_test(&mut self, worker_id: usize) -> Option<RunningTest> {
        self.in_flight.remove(&worker_id)
    }
}
