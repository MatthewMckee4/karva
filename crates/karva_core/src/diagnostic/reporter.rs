use std::fmt::Write;

use colored::Colorize;
use karva_project::VerbosityLevel;
use pyo3::marker::Ungil;

use crate::{IndividualTestResultKind, Printer};

/// A reporter for test execution time logging to the user.
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
    printer: Printer,
}

impl Default for TestCaseReporter {
    fn default() -> Self {
        Self::new(Printer::new(VerbosityLevel::default(), false))
    }
}

impl TestCaseReporter {
    pub const fn new(printer: Printer) -> Self {
        Self { printer }
    }
}

impl Reporter for TestCaseReporter {
    fn report_test_case_result(&self, test_name: &str, result_kind: IndividualTestResultKind) {
        let mut stdout = self.printer.stream_for_test_result().lock();

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
