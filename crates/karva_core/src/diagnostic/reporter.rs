use std::{
    io::Write,
    sync::{Arc, Mutex},
};

use colored::Colorize;
use pyo3::marker::Ungil;

use crate::runner::diagnostic::IndividualTestResultKind;

/// A progress reporter.
pub trait Reporter: Send + Sync + Ungil {
    /// Report the completion of a given test.
    fn report_test_case_result(&self, test_name: &str, result_kind: IndividualTestResultKind);
}

/// A no-op implementation of [`Reporter`].
#[derive(Default)]
pub struct DummyReporter;

impl Reporter for DummyReporter {
    fn report_test_case_result(&self, _test_name: &str, _result_kind: IndividualTestResultKind) {}
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

        let log_start = format!("test {test_name} ...");

        let rest = match result_kind {
            IndividualTestResultKind::Passed => "ok".green().to_string(),
            IndividualTestResultKind::Failed => "FAILED".red().to_string(),
            IndividualTestResultKind::Skipped { reason } => {
                let skipped_string = "skipped".yellow().to_string();
                if let Some(reason) = reason {
                    format!("{skipped_string}: {reason}")
                } else {
                    skipped_string
                }
            }
        };

        writeln!(stdout, "{log_start} {rest}").ok();
    }
}
