//! Event ordering and controller-owned test state.
//!
//! Reader threads only decode complete IPC frames. This module applies those
//! frames serially so result aggregation and active-test attribution have one
//! owner.

use std::collections::{HashMap, HashSet};
use std::process::ExitStatus;
use std::time::Duration;

use anyhow::Result;
use karva_diagnostic::AggregatedResults;
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

    /// Completed test membership retained in its smallest useful form when
    /// durations are not needed by cache or terminal output.
    completed_test_tracking: CompletedTestTracking,

    /// Synthetic crash results deferred until recovery no longer needs exact
    /// `TestFinished` membership.
    crashed_tests: Vec<CrashedTest>,

    /// Completed case bodies retained for final report formats.
    result_retention: TestResultRetention,
}

/// Selects the representation needed for crash-recovery membership checks.
#[derive(Default)]
enum CompletedTestTracking {
    /// Use the retained duration-map keys as exact completion membership.
    #[default]
    Durations,

    /// Retain completion identities without per-case durations.
    Compact(CompactCompletedKeys),
}

/// Stores exact completed keys without retaining a duration per case.
#[derive(Default)]
struct CompactCompletedKeys {
    /// Complete keys that cannot be represented as a canonical case index.
    plain: HashSet<Box<str>>,

    /// Canonical case indices grouped under one allocated function key.
    parameterized: HashMap<Box<str>, HashSet<usize>>,
}

impl CompactCompletedKeys {
    /// Records one completed function or indexed parameter case.
    fn insert(&mut self, cache_key: &TestCacheKey) {
        let Some(index) = cache_key.parameter_case_index() else {
            self.plain.insert(cache_key.as_str().into());
            return;
        };

        let function = cache_key.test_function_name();
        if let Some(indices) = self.parameterized.get_mut(function) {
            indices.insert(index);
            return;
        }

        self.parameterized
            .insert(function.into(), HashSet::from([index]));
    }

    /// Rebuilds full cache keys only when a replacement worker needs them.
    fn materialize(&self) -> HashSet<TestCacheKey> {
        let parameterized_count = self.parameterized.values().map(HashSet::len).sum::<usize>();
        let mut keys = HashSet::with_capacity(self.plain.len() + parameterized_count);
        keys.extend(
            self.plain
                .iter()
                .map(|key| TestCacheKey::function_name(key)),
        );
        for (function, indices) in &self.parameterized {
            keys.extend(
                indices
                    .iter()
                    .map(|index| TestCacheKey::parameter_case_name(function, *index)),
            );
        }
        keys
    }
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
        retain_durations: bool,
    ) -> Self {
        let test_case_capacity = match result_retention {
            TestResultRetention::FailuresAndRetries => 0,
            TestResultRetention::All => test_capacity,
        };
        Self {
            expected_workers: HashSet::new(),
            completed_workers: HashSet::new(),
            results: AggregatedResults::with_capacities(
                if retain_durations { test_capacity } else { 0 },
                test_case_capacity,
            ),
            completed_test_tracking: if retain_durations {
                CompletedTestTracking::Durations
            } else {
                CompletedTestTracking::Compact(CompactCompletedKeys::default())
            },
            crashed_tests: Vec::new(),
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
                WorkerEvent::TestFinished { cache_key, result } => {
                    if let CompletedTestTracking::Compact(completed) =
                        &mut self.completed_test_tracking
                    {
                        completed.insert(&cache_key);
                    }
                    self.results.register_rendered_test_case(
                        cache_key,
                        *result,
                        matches!(self.result_retention, TestResultRetention::All),
                        matches!(
                            &self.completed_test_tracking,
                            CompletedTestTracking::Durations
                        ),
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
        match &self.completed_test_tracking {
            CompletedTestTracking::Durations => self.results.durations.keys().cloned().collect(),
            CompletedTestTracking::Compact(completed) => completed.materialize(),
        }
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
            results.register_crashed_test(
                &crashed.name,
                crashed.cache_key,
                crashed.duration,
                &crashed.termination,
                &crashed.stderr,
                matches!(
                    &self.completed_test_tracking,
                    CompletedTestTracking::Durations
                ),
            );
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::time::Duration;

    use karva_python_semantic::TestCacheKey;

    use super::{CompactCompletedKeys, EventDispatcher};

    #[test]
    fn compact_completed_keys_materialize_exact_cases() {
        let mut completed = CompactCompletedKeys::default();
        for key in [
            "module::test_case[0]",
            "module::test_case[2]",
            "module::test_case[2]",
            "module::test_case[01]",
            "module::test_plain",
            "module::test[not-an-index]",
        ] {
            completed.insert(&TestCacheKey::function_name(key));
        }

        assert_eq!(
            completed.materialize(),
            HashSet::from([
                TestCacheKey::function_name("module::test_case[0]"),
                TestCacheKey::function_name("module::test_case[2]"),
                TestCacheKey::function_name("module::test_case[01]"),
                TestCacheKey::function_name("module::test_plain"),
                TestCacheKey::function_name("module::test[not-an-index]"),
            ])
        );
    }

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
