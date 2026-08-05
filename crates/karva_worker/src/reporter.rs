use std::time::Duration;

use karva_diagnostic::{IndividualTestResultKind, Reporter, TestCaseReporter};
use karva_ipc::WorkerState;
use karva_python_semantic::QualifiedTestName;

/// Sends lifecycle state to the controller while preserving terminal output.
pub struct WorkerReporter {
    output: TestCaseReporter,
    state: WorkerState,
}

impl WorkerReporter {
    pub fn new(output: TestCaseReporter, state: WorkerState) -> Self {
        Self { output, state }
    }
}

impl Reporter for WorkerReporter {
    fn report_test_case_result(
        &self,
        test_name: &QualifiedTestName,
        result_kind: IndividualTestResultKind,
        duration: Duration,
    ) {
        self.output
            .report_test_case_result(test_name, result_kind, duration);
    }

    fn report_test_attempt(
        &self,
        test_name: &QualifiedTestName,
        attempt: u32,
        result_kind: IndividualTestResultKind,
        duration: Duration,
    ) {
        self.output
            .report_test_attempt(test_name, attempt, result_kind, duration);
    }

    fn report_test_slow(&self, test_name: &QualifiedTestName, duration: Duration) {
        self.output.report_test_slow(test_name, duration);
    }

    fn report_test_started(&self, test_name: &QualifiedTestName) {
        self.state.start(test_name.to_string());
    }

    fn report_test_finished(&self, _test_name: &QualifiedTestName) {
        self.state.finish();
    }
}
