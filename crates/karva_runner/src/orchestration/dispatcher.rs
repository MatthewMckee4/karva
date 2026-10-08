//! Event ordering and controller-owned test state.
//!
//! Reader threads only decode complete IPC frames. This module applies those
//! frames serially so result aggregation and active-test attribution have one
//! owner.

use std::collections::{HashMap, HashSet};
use std::process::ExitStatus;
use std::time::Duration;

use anyhow::Result;
use karva_diagnostic::{AggregatedResults, TestCaseAttempt, TestCaseResult};
use karva_ipc::{ControllerServer, WorkerCheckpoint, WorkerEvent};
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

    /// Results and diagnostics aggregated across every worker generation.
    results: AggregatedResults,

    /// Synthetic crash results deferred until recovery no longer needs the
    /// duration map as exact `TestFinished` membership.
    crashed_tests: Vec<CrashedTest>,

    /// Completed case bodies retained for final report formats.
    result_retention: TestResultRetention,

    /// Timeout events waiting for their worker generation to be reaped.
    timed_out_workers: HashMap<usize, TimedOutWorker>,

    /// Earlier attempts awaiting a final result from a replacement interpreter.
    timeout_history: HashMap<TestCacheKey, TimeoutHistory>,
}

/// Flushed timeout event, retained until process exit establishes safe retry isolation.
struct TimedOutWorker {
    cache_key: TestCacheKey,
    result: TestCaseResult,
    fail_on_flaky: bool,
    junit_fail_on_flaky: bool,
}

/// Attempt history retained only for cases still eligible to retry.
struct TimeoutHistory {
    attempts: Vec<TestCaseAttempt>,
    next_attempt: u32,
    max_attempts: u32,
    fail_on_flaky: bool,
    junit_fail_on_flaky: bool,
}

/// Unexpected test termination retained until crash recovery completes.
struct CrashedTest {
    /// Last refined display name reported by the worker.
    name: String,

    /// Stable case identity excluded from committed-result membership.
    cache_key: TestCacheKey,

    /// Time from the latest start checkpoint until process exit.
    duration: Duration,

    /// Platform-specific process termination description.
    termination: String,

    /// Bounded worker stderr included in the final diagnostic.
    stderr: String,
}

/// Unexpected worker exit plus state needed to report and retry it.
#[derive(Debug)]
pub(super) struct CrashedWorker {
    /// Worker generation that exited unexpectedly.
    pub(super) id: usize,

    /// Remaining selection eligible for replacement execution.
    pub(super) partition: Partition,

    /// Exit status captured before process reaping.
    pub(super) status: ExitStatus,

    /// Bounded stderr diagnostic captured from the worker.
    pub(super) stderr: String,

    /// Final checkpoint plus whether forced reader shutdown could have lost a frame.
    pub(super) checkpoint: CrashCheckpoint,

    /// Whether the process authenticated its controller connection before exit.
    pub(super) controller_authenticated: bool,
}

/// Active-test state recovered from one failed worker connection.
#[derive(Debug)]
pub(super) enum CrashCheckpoint {
    /// The reader reached EOF before recovery inspected its final state.
    Complete(Option<WorkerCheckpoint>),

    /// The reader was force-closed before its final state could be trusted.
    ///
    /// The last decoded checkpoint is diagnostic context only; later frames
    /// may have completed it or started another test.
    DrainLimited(Option<WorkerCheckpoint>),
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
            results: AggregatedResults::with_capacities(test_capacity, test_case_capacity),
            crashed_tests: Vec::new(),
            result_retention,
            timed_out_workers: HashMap::new(),
            timeout_history: HashMap::new(),
        }
    }

    /// Admits one worker generation before its process can send events.
    pub(super) fn register_worker(&mut self, worker_id: usize) {
        self.expected_workers.insert(worker_id);
    }

    /// Applies every queued worker event to controller-owned run state.
    pub(super) fn dispatch_pending(&mut self, server: &mut ControllerServer) -> Result<()> {
        server.accept_pending()?;
        let queued_messages = server.queued_message_count();
        for _ in 0..queued_messages {
            let Some(message) = server.try_recv()? else {
                break;
            };
            let worker_id = message.worker_id;
            if !self.expected_workers.contains(&worker_id) {
                anyhow::bail!("unknown Karva worker {worker_id} sent a controller event");
            }
            match *message.event {
                WorkerEvent::TestSlow => self.results.register_slow_test(),
                WorkerEvent::TestTimedOut {
                    cache_key,
                    result,
                    fail_on_flaky,
                    junit_fail_on_flaky,
                } => {
                    if self
                        .timed_out_workers
                        .insert(
                            worker_id,
                            TimedOutWorker {
                                cache_key,
                                result: *result,
                                fail_on_flaky,
                                junit_fail_on_flaky,
                            },
                        )
                        .is_some()
                    {
                        anyhow::bail!(
                            "Karva worker {worker_id} reported more than one hard timeout"
                        );
                    }
                }
                WorkerEvent::TestFinished { cache_key, result } => {
                    let result = if let Some(history) = self.timeout_history.remove(&cache_key) {
                        Box::new(result.with_previous_attempts(
                            history.attempts,
                            history.next_attempt,
                            history.max_attempts,
                            history.fail_on_flaky,
                            history.junit_fail_on_flaky,
                        ))
                    } else {
                        result
                    };
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

    /// Builds exact `TestFinished` membership only when recovery needs it.
    ///
    /// Deferred synthetic crash results are intentionally absent.
    pub(super) fn completed_test_keys(&self) -> HashSet<TestCacheKey> {
        self.results.durations.keys().cloned().collect()
    }

    /// Whether a native deadline handler reported this worker exit in advance.
    pub(super) fn worker_timed_out(&self, worker_id: usize) -> bool {
        self.timed_out_workers.contains_key(&worker_id)
    }

    /// Whether a worker delivered its terminal event exactly once.
    pub(super) fn worker_completed(&self, worker_id: usize) -> bool {
        self.completed_workers.contains(&worker_id)
    }

    /// Returns sorted generations that never delivered their terminal event.
    pub(super) fn missing_workers(&self) -> Vec<usize> {
        let mut missing = self
            .expected_workers
            .difference(&self.completed_workers)
            .copied()
            .collect::<Vec<_>>();
        missing.sort_unstable();
        missing
    }

    /// Resolves a flushed timeout only after its interpreter has exited.
    pub(super) fn recover_timeout(
        &mut self,
        worker_id: usize,
        stderr: &str,
    ) -> Option<(TestCacheKey, Option<u32>)> {
        let timed_out = self.timed_out_workers.remove(&worker_id)?;
        let retry = timed_out.result.retry()?;
        let attempt_number = retry.attempts();
        let max_attempts = retry.max_attempts();
        let mut result = timed_out.result;
        result.append_captured_stderr(stderr);
        let history = self.timeout_history.remove(&timed_out.cache_key);
        let previous = history.map_or_else(Vec::new, |history| history.attempts);
        if attempt_number < max_attempts {
            let mut attempts = previous;
            attempts.extend_from_slice(result.attempts());
            self.timeout_history.insert(
                timed_out.cache_key.clone(),
                TimeoutHistory {
                    attempts,
                    next_attempt: attempt_number + 1,
                    max_attempts,
                    fail_on_flaky: timed_out.fail_on_flaky,
                    junit_fail_on_flaky: timed_out.junit_fail_on_flaky,
                },
            );
            Some((timed_out.cache_key, Some(attempt_number + 1)))
        } else {
            let result = result.with_previous_attempts(
                previous,
                attempt_number,
                max_attempts,
                timed_out.fail_on_flaky,
                timed_out.junit_fail_on_flaky,
            );
            self.results.register_rendered_test_case(
                timed_out.cache_key.clone(),
                result,
                matches!(self.result_retention, TestResultRetention::All),
            );
            Some((timed_out.cache_key, None))
        }
    }

    /// Adds a run-level diagnostic when no active test checkpoint survived an exit.
    pub(super) fn register_worker_exit(&mut self, summary: &str, recovery: &str, stderr: &str) {
        self.results.register_worker_exit(summary, recovery, stderr);
    }

    /// Defers one synthetic crash result so it cannot look committed to recovery.
    pub(super) fn register_crashed_test(
        &mut self,
        name: &str,
        cache_key: TestCacheKey,
        duration: Duration,
        termination: &str,
        stderr: &str,
    ) {
        self.crashed_tests.push(CrashedTest {
            name: name.to_string(),
            cache_key,
            duration,
            termination: termination.to_string(),
            stderr: stderr.to_string(),
        });
    }

    /// Counts received failures plus crash results not yet materialized.
    pub(super) fn failure_count(&self) -> u32 {
        let failures = self
            .results
            .stats()
            .failed()
            .saturating_add(self.results.stats().errors())
            .saturating_add(self.crashed_tests.len());
        u32::try_from(failures).unwrap_or(u32::MAX)
    }

    /// Materializes deferred crash results after recovery has finished.
    pub(super) fn take_results(&mut self) -> AggregatedResults {
        let mut results = std::mem::take(&mut self.results);
        for crashed in self.crashed_tests.drain(..) {
            if let Some(history) = self.timeout_history.remove(&crashed.cache_key) {
                let result = TestCaseResult::crashed(
                    &crashed.name,
                    crashed.duration,
                    &crashed.termination,
                    &crashed.stderr,
                )
                .with_previous_attempts(
                    history.attempts,
                    history.next_attempt,
                    history.max_attempts,
                    history.fail_on_flaky,
                    history.junit_fail_on_flaky,
                );
                results.register_rendered_test_case(crashed.cache_key, result, true);
            } else {
                results.register_crashed_test(
                    &crashed.name,
                    crashed.cache_key,
                    crashed.duration,
                    &crashed.termination,
                    &crashed.stderr,
                );
            }
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use karva_python_semantic::TestCacheKey;

    use super::EventDispatcher;

    #[test]
    fn crashed_test_is_not_committed_until_recovery_finishes() {
        let cache_key = TestCacheKey::function_name("test_module::test_case[1]");
        let mut dispatcher = EventDispatcher::default();

        dispatcher.register_crashed_test(
            "test_module::test_case(value=1)",
            cache_key.clone(),
            Duration::from_millis(5),
            "exit code 17",
            "worker stderr",
        );

        assert!(dispatcher.completed_test_keys().is_empty());
        assert_eq!(dispatcher.failure_count(), 1);

        let results = dispatcher.take_results();
        assert_eq!(results.stats().errors(), 1);
        assert_eq!(results.durations[&cache_key], Duration::from_millis(5));
    }
}
