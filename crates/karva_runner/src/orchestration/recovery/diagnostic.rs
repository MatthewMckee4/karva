//! Runner-owned context for unattributed worker-exit diagnostics.

/// Last controller-observed lifecycle stage before an unattributed exit.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum WorkerExitStage {
    /// The process exited before authenticating its controller connection.
    BeforeControllerAuthentication,

    /// The worker authenticated, but no test checkpoint remained active.
    OutsideActiveTest,
}

/// Recovery outcome chosen from the failed assignment's committed progress.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) enum WorkerExitRecovery {
    /// Run the remaining selection in another worker generation.
    Retrying {
        /// Results from the failed assignment already committed to the controller.
        completed_results: usize,

        /// Unstarted selectors assigned to the replacement worker.
        pending_selections: usize,
    },

    /// No selection remained after filtering committed results.
    Complete {
        /// Results from the failed assignment already committed to the controller.
        completed_results: usize,
    },

    /// Stop after a replacement worker committed no additional results.
    Stalled {
        /// Results from the failed assignment already committed to the controller.
        completed_results: usize,
    },

    /// Do not start a replacement because the run reached its failure budget.
    FailureLimit {
        /// Results from the failed assignment already committed to the controller.
        completed_results: usize,

        /// Unstarted selectors discarded when the run stopped.
        pending_selections: usize,
    },
}

/// Validated diagnostic context after the runner has selected a recovery action.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(super) struct WorkerExitContext {
    /// Worker generation that exited unexpectedly.
    worker_id: usize,

    /// Last lifecycle stage established by controller state.
    stage: WorkerExitStage,

    /// Recovery action selected from committed result state.
    recovery: WorkerExitRecovery,
}

impl WorkerExitContext {
    /// Captures the finalized runner-owned state used for user-facing output.
    pub(super) fn new(
        worker_id: usize,
        stage: WorkerExitStage,
        recovery: WorkerExitRecovery,
    ) -> Self {
        Self {
            worker_id,
            stage,
            recovery,
        }
    }

    /// Describes where the worker was in its lifecycle when it exited.
    pub(super) fn summary(self, termination: &str) -> String {
        let stage = match self.stage {
            WorkerExitStage::BeforeControllerAuthentication => {
                "during startup before controller authentication"
            }
            WorkerExitStage::OutsideActiveTest => "with no active test checkpoint",
        };
        format!(
            "Worker {} terminated with {termination} {stage}",
            self.worker_id
        )
    }

    /// Explains retained progress and the replacement decision.
    pub(super) fn recovery_message(self) -> String {
        match self.recovery {
            WorkerExitRecovery::Retrying {
                completed_results,
                pending_selections,
            } => format!(
                "Karva preserved {} from this assignment and is retrying {} in a replacement worker.",
                counted(completed_results, "completed test result"),
                counted(pending_selections, "unstarted test selection"),
            ),
            WorkerExitRecovery::Complete { completed_results } => format!(
                "Karva preserved {} from this assignment; no unstarted test selection remained. The worker exited after test execution, during cleanup or shutdown.",
                counted(completed_results, "completed test result"),
            ),
            WorkerExitRecovery::Stalled { completed_results } => format!(
                "Karva preserved {} from this assignment and stopped retrying because the replacement worker committed no new result.",
                counted(completed_results, "completed test result"),
            ),
            WorkerExitRecovery::FailureLimit {
                completed_results,
                pending_selections,
            } => format!(
                "Karva preserved {} from this assignment and did not retry {} because `--max-fail` was reached.",
                counted(completed_results, "completed test result"),
                counted(pending_selections, "unstarted test selection"),
            ),
        }
    }
}

/// Formats a counted noun with the correct singular or plural suffix.
fn counted(count: usize, singular: &str) -> String {
    let suffix = if count == 1 { "" } else { "s" };
    format!("{count} {singular}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_startup_and_failure_limit_context() {
        let context = WorkerExitContext::new(
            3,
            WorkerExitStage::BeforeControllerAuthentication,
            WorkerExitRecovery::FailureLimit {
                completed_results: 0,
                pending_selections: 2,
            },
        );

        assert_eq!(
            context.summary("exit code 9"),
            "Worker 3 terminated with exit code 9 during startup before controller authentication"
        );
        assert_eq!(
            context.recovery_message(),
            "Karva preserved 0 completed test results from this assignment and did not retry 2 unstarted test selections because `--max-fail` was reached."
        );
    }

    #[test]
    fn reports_cleanup_after_completed_results() {
        let context = WorkerExitContext::new(
            4,
            WorkerExitStage::OutsideActiveTest,
            WorkerExitRecovery::Complete {
                completed_results: 1,
            },
        );

        assert_eq!(
            context.summary("exit code 27"),
            "Worker 4 terminated with exit code 27 with no active test checkpoint"
        );
        assert_eq!(
            context.recovery_message(),
            "Karva preserved 1 completed test result from this assignment; no unstarted test selection remained. The worker exited after test execution, during cleanup or shutdown."
        );
    }
}
