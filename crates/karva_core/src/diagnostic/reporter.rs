use std::{
    io::Write,
    sync::{Arc, Mutex},
};

use pyo3::marker::Ungil;

use crate::{diagnostic::DiscoveryDiagnostic, runner::diagnostic::IndividualTestResultKind};

/// A progress reporter.
pub trait Reporter: Send + Sync + Ungil {
    /// Report the completion of a given test.
    fn report_test_case_result(&self, test_name: &str, result_kind: IndividualTestResultKind);

    /// Log the number of test cases that will be run.
    fn log_test_count(&self, test_count: usize);

    fn report_discovery_diagnostics(&self, diagnostics: &[DiscoveryDiagnostic]);
}

/// A no-op implementation of [`Reporter`].
#[derive(Default)]
pub struct DummyReporter;

impl Reporter for DummyReporter {
    fn report_test_case_result(&self, _test_name: &str, _result_kind: IndividualTestResultKind) {}

    fn log_test_count(&self, _test_count: usize) {}

    fn report_discovery_diagnostics(&self, _diagnostics: &[DiscoveryDiagnostic]) {}
}

/// A reporter that outputs test results to stdout as they complete.
pub struct TestCaseReporter {
    output: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Default for TestCaseReporter {
    fn default() -> Self {
        Self::new(Arc::new(Mutex::new(Box::new(std::io::stdout()))))
    }
}

impl TestCaseReporter {
    pub fn new(output: Arc<Mutex<Box<dyn Write + Send>>>) -> Self {
        Self { output }
    }
}

impl Reporter for TestCaseReporter {
    fn report_test_case_result(&self, test_name: &str, result_kind: IndividualTestResultKind) {
        let mut stdout = self.output.lock().unwrap();
        match result_kind {
            IndividualTestResultKind::Passed => {
                writeln!(stdout, "test {test_name} ... ok").ok();
            }
            IndividualTestResultKind::Failed => {
                writeln!(stdout, "test {test_name} ... FAILED").ok();
            }
            IndividualTestResultKind::Skipped { reason } => {
                if let Some(reason) = reason {
                    writeln!(stdout, "test {test_name} ... skipped: {reason}").ok();
                } else {
                    writeln!(stdout, "test {test_name} ... skipped").ok();
                }
            }
        }
    }

    fn log_test_count(&self, test_count: usize) {
        let mut stdout = self.output.lock().unwrap();
        writeln!(
            stdout,
            "running {test_count} test{}",
            if test_count == 1 { "" } else { "s" }
        )
        .ok();
    }

    fn report_discovery_diagnostics(&self, diagnostics: &[DiscoveryDiagnostic]) {
        if diagnostics.is_empty() {
            return;
        }

        let mut stdout = self.output.lock().unwrap();

        writeln!(stdout, "discovery diagnostics:").ok();

        writeln!(stdout).ok();

        for diagnostic in diagnostics {
            writeln!(stdout, "{}", diagnostic.display()).ok();
        }
    }
}
