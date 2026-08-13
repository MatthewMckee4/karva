//! Worker supervision state and its focused process/event façade.

use std::collections::HashSet;
use std::time::Duration;

use anyhow::Result;
use karva_diagnostic::AggregatedResults;
use karva_ipc::ControllerServer;
use karva_python_semantic::TestCacheKey;

use super::config::TestResultRetention;
use super::dispatcher::EventDispatcher;
use super::worker::Worker;
use crate::partition::Partition;

mod polling;

pub(super) use polling::WaitOutcome;

/// Owns live workers and serializes their process and event lifecycle.
pub(super) struct WorkerSupervisor {
    /// Child generations that have not completed process and stream cleanup.
    workers: Vec<Worker>,

    /// Serialized event state shared with crash recovery and cancellation.
    dispatcher: EventDispatcher,
}

impl WorkerSupervisor {
    /// Borrows live workers for read-only process inspection.
    pub(super) fn workers(&self) -> &[Worker] {
        &self.workers
    }

    /// Borrows live workers for process cleanup.
    pub(super) fn workers_mut(&mut self) -> &mut [Worker] {
        &mut self.workers
    }

    /// Borrows workers and event state together for coordinated termination.
    pub(super) fn worker_state(&mut self) -> (&mut [Worker], &EventDispatcher) {
        (&mut self.workers, &self.dispatcher)
    }

    /// Drops workers whose process and stream cleanup has completed.
    pub(super) fn clear_workers(&mut self) {
        self.workers.clear();
    }

    /// Whether any generation still needs process or stream cleanup.
    pub(super) fn has_workers(&self) -> bool {
        !self.workers.is_empty()
    }

    /// Whether a generation delivered its terminal event before disconnecting.
    pub(super) fn worker_completed(&self, worker_id: usize) -> bool {
        self.dispatcher.worker_completed(worker_id)
    }

    /// Returns exact completed-case membership for replacement-worker filtering.
    pub(super) fn completed_test_keys(&self) -> HashSet<TestCacheKey> {
        self.dispatcher.completed_test_keys()
    }

    /// Removes a failed generation from the set allowed to send current events.
    pub(super) fn abandon_worker(&mut self, worker_id: usize) {
        self.dispatcher.abandon_worker(worker_id);
    }

    /// Records a run-level diagnostic for an exit with no active test checkpoint.
    pub(super) fn register_worker_exit(
        &mut self,
        worker_id: usize,
        termination: &str,
        stderr: &str,
    ) {
        self.dispatcher
            .register_worker_exit(worker_id, termination, stderr);
    }

    /// Defers an active-test crash result until recovery no longer reads completion state.
    pub(super) fn register_crashed_test(
        &mut self,
        name: &str,
        cache_key: TestCacheKey,
        duration: Duration,
        termination: &str,
        stderr: &str,
    ) {
        self.dispatcher
            .register_crashed_test(name, cache_key, duration, termination, stderr);
    }

    /// Counts committed failures and deferred crash results for fail-fast.
    pub(super) fn failure_count(&self) -> u32 {
        self.dispatcher.failure_count()
    }

    /// Joins all IPC readers and dispatches their final complete frames.
    pub(super) fn finish_events(&mut self, server: &mut ControllerServer) -> Result<()> {
        self.dispatcher.finish(server)
    }

    /// Applies every controller event currently available without blocking.
    pub(super) fn dispatch_events(&mut self, server: &mut ControllerServer) -> Result<()> {
        self.dispatcher.dispatch_pending(server)
    }

    /// Takes final aggregation after materializing deferred crash results.
    pub(super) fn take_results(&mut self) -> AggregatedResults {
        self.dispatcher.take_results()
    }

    /// Allocates empty supervision state sized for the selected reporting mode.
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
    /// Registers a child generation with both event dispatch and process supervision.
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
}
