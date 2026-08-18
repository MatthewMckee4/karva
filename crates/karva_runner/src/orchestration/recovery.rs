//! Controller-side crash attribution and replacement-worker planning.
//!
//! This module turns one batch of exited worker generations into committed
//! diagnostics plus filtered replacement assignments. It reads only results
//! already delivered to the controller, so every retry decision is based on
//! durable progress.

use anyhow::Result;
use karva_ipc::ControllerServer;
use karva_logging::Printer;
use karva_metadata::MaxFail;

use super::CANCELLATION_EVENT_SETTLE;
use super::dispatcher::{CrashCheckpoint, CrashedWorker};
use super::output::{print_crashed_test, termination_description};
use super::supervision::WorkerSupervisor;
use crate::partition::{CompletedTestIndex, Partition, UnattributedCrashRecovery};

mod diagnostic;

use diagnostic::{WorkerExitContext, WorkerExitRecovery, WorkerExitStage};

/// Recovery state retained until the batch-wide failure budget is known.
enum PendingCrashRecovery {
    /// Test-attributed crash already committed as a test result.
    Active(Option<Partition>),

    /// Exit without an active checkpoint, awaiting its final diagnostic.
    Unattributed(CrashedWorker),
}

/// Replacement assignments and stop state produced by one crash batch.
pub(super) struct CrashRecoveryPlan {
    /// Filtered work that remains safe to execute in new worker generations.
    replacements: Vec<Partition>,

    /// Whether recorded failures exhausted `--max-fail` before replacement.
    failure_limit_reached: bool,
}

impl CrashRecoveryPlan {
    /// Whether the caller must stop instead of spawning replacements.
    pub(super) fn failure_limit_reached(&self) -> bool {
        self.failure_limit_reached
    }

    /// Consumes the plan and returns replacement assignments in crash order.
    pub(super) fn into_replacements(self) -> Vec<Partition> {
        self.replacements
    }
}

/// Commits crash diagnostics and plans replacements from durable event state.
pub(super) fn recover_crashed_workers(
    worker_manager: &mut WorkerSupervisor,
    controller: &mut ControllerServer,
    crashed_workers: Vec<CrashedWorker>,
    max_fail: MaxFail,
    printer: Printer,
) -> Result<CrashRecoveryPlan> {
    worker_manager.dispatch_events(controller)?;
    let completed_test_keys = worker_manager.completed_test_keys();
    let completed_index = CompletedTestIndex::new(&completed_test_keys);
    let mut pending_recovery = Vec::with_capacity(crashed_workers.len());

    for crashed_worker in crashed_workers {
        worker_manager.abandon_worker(crashed_worker.id);
        match &crashed_worker.checkpoint {
            CrashCheckpoint::Complete(Some(active)) => {
                let pending = recover_active_test(
                    worker_manager,
                    &crashed_worker,
                    active.clone(),
                    &completed_index,
                    printer,
                );
                pending_recovery.push(PendingCrashRecovery::Active(pending));
            }
            CrashCheckpoint::Complete(None) | CrashCheckpoint::DrainLimited(_) => {
                pending_recovery.push(PendingCrashRecovery::Unattributed(crashed_worker));
            }
        }
    }

    let failures = worker_manager.failure_count();
    let failure_limit_reached = max_fail.is_exceeded_by(failures);
    let mut replacements = Vec::new();
    for recovery in pending_recovery {
        match recovery {
            PendingCrashRecovery::Active(Some(pending)) if !failure_limit_reached => {
                replacements.push(pending);
            }
            PendingCrashRecovery::Active(_) => {}
            PendingCrashRecovery::Unattributed(crashed_worker) => {
                if let Some(pending) = recover_unattributed_exit(
                    worker_manager,
                    &crashed_worker,
                    &completed_index,
                    failure_limit_reached,
                ) {
                    replacements.push(pending);
                }
            }
        }
    }

    Ok(CrashRecoveryPlan {
        replacements,
        failure_limit_reached,
    })
}

/// Records one active test crash and filters work for its replacement.
fn recover_active_test(
    worker_manager: &mut WorkerSupervisor,
    crashed_worker: &CrashedWorker,
    active: karva_ipc::WorkerCheckpoint,
    completed_tests: &CompletedTestIndex<'_>,
    printer: Printer,
) -> Option<Partition> {
    let termination = termination_description(crashed_worker.status);
    let duration = active.started.elapsed();
    let name = active.name;
    let cache_key = active.cache_key;
    print_crashed_test(printer, &name, duration);
    let pending = crashed_worker
        .partition
        .pending_after_test_crash(completed_tests, &cache_key);
    worker_manager.register_crashed_test(
        &name,
        cache_key,
        duration,
        &termination,
        &crashed_worker.stderr,
    );
    (!pending.is_empty()).then_some(pending)
}

/// Records one unattributed exit and returns work allowed to be retried.
fn recover_unattributed_exit(
    worker_manager: &mut WorkerSupervisor,
    crashed_worker: &CrashedWorker,
    completed_tests: &CompletedTestIndex<'_>,
    failure_limit_reached: bool,
) -> Option<Partition> {
    let (stage, checkpoint_drain_limited) = match &crashed_worker.checkpoint {
        CrashCheckpoint::DrainLimited(active) => (
            WorkerExitStage::ControllerEventDrainLimit {
                limit: CANCELLATION_EVENT_SETTLE,
                last_checkpoint: active.as_ref().map(|active| active.name.clone()),
            },
            true,
        ),
        CrashCheckpoint::Complete(_) if crashed_worker.controller_authenticated => {
            (WorkerExitStage::OutsideActiveTest, false)
        }
        CrashCheckpoint::Complete(_) => (WorkerExitStage::BeforeControllerAuthentication, false),
    };
    let (recovery, pending) = match crashed_worker
        .partition
        .recover_unattributed_crash(completed_tests)
    {
        UnattributedCrashRecovery::Retry {
            pending,
            completed_results,
        } => {
            let pending_selections = pending.test_count();
            if checkpoint_drain_limited {
                (
                    WorkerExitRecovery::Indeterminate {
                        completed_results,
                        pending_selections,
                    },
                    None,
                )
            } else if failure_limit_reached {
                (
                    WorkerExitRecovery::FailureLimit {
                        completed_results,
                        pending_selections,
                    },
                    None,
                )
            } else {
                (
                    WorkerExitRecovery::Retrying {
                        completed_results,
                        pending_selections,
                    },
                    Some(pending),
                )
            }
        }
        UnattributedCrashRecovery::Complete { completed_results } => {
            (WorkerExitRecovery::Complete { completed_results }, None)
        }
        UnattributedCrashRecovery::Stalled { completed_results } => {
            (WorkerExitRecovery::Stalled { completed_results }, None)
        }
    };
    let context = WorkerExitContext::new(crashed_worker.id, stage, recovery);
    let termination = termination_description(crashed_worker.status);
    worker_manager.register_worker_exit(
        &context.summary(&termination),
        &context.recovery_message(),
        &crashed_worker.stderr,
    );
    pending
}
