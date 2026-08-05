use std::io::Write;
use std::time::Duration;

use colored::Colorize;
use karva_logging::time::format_duration_bracketed;
use karva_logging::{Printer, StatusLevel};
use karva_python_semantic::{QualifiedTestName, TestCacheKey};

use crate::Diagnostic;
use crate::result::{IndividualTestResultKind, TestExecutionResult};

/// A reporter for test execution time logging to the user.
pub trait Reporter: Send + Sync {
    /// Report the completion of a non-retried test.
    fn report_test_case_result(
        &self,
        test_name: &QualifiedTestName,
        result_kind: IndividualTestResultKind,
        duration: Duration,
    );

    /// Report one attempt of a retried test as it completes.
    ///
    /// `attempt` is 1-indexed (the first attempt is `1`). For a retried test
    /// this is called once per attempt — including the final one — and the
    /// runner does NOT additionally call [`Self::report_test_case_result`].
    /// Default no-op for reporters that don't surface attempt-level detail.
    fn report_test_attempt(
        &self,
        test_name: &QualifiedTestName,
        attempt: u32,
        result_kind: IndividualTestResultKind,
        duration: Duration,
    ) {
        let _ = (test_name, attempt, result_kind, duration);
    }

    /// Report that a test exceeded the configured slow-test threshold.
    ///
    /// Emitted in addition to (and ahead of) the regular result line. Default
    /// no-op for reporters that don't surface slow-test detail.
    fn report_test_slow(&self, test_name: &QualifiedTestName, duration: Duration) {
        let _ = (test_name, duration);
    }

    /// Called immediately before a test starts executing.
    ///
    /// Used by reporters that track in-flight tests for cancellation
    /// reporting; default is a no-op.
    fn report_test_started(&self, test_name: &QualifiedTestName) {
        let _ = test_name;
    }

    /// Called with the final result after each test completes.
    fn report_test_completed(&self, cache_key: &TestCacheKey, result: &TestExecutionResult) {
        let _ = (cache_key, result);
    }

    /// Called when a run-level diagnostic is recorded.
    fn report_run_diagnostic(&self, diagnostic: &Diagnostic) {
        let _ = diagnostic;
    }
}

fn show_for_status_level(level: StatusLevel, kind: &IndividualTestResultKind) -> bool {
    // Levels are cumulative, like nextest: each level shows itself plus all
    // earlier levels. The `Slow` line is gated separately in
    // `report_test_slow`, so `Slow` here acts the same as `Retry`.
    match level {
        StatusLevel::None => false,
        StatusLevel::Fail | StatusLevel::Retry | StatusLevel::Slow => {
            matches!(
                kind,
                IndividualTestResultKind::Failed | IndividualTestResultKind::Error
            )
        }
        StatusLevel::Pass => matches!(
            kind,
            IndividualTestResultKind::Failed
                | IndividualTestResultKind::Error
                | IndividualTestResultKind::Passed
        ),
        StatusLevel::Skip | StatusLevel::All => true,
    }
}

/// A no-op implementation of [`Reporter`].
#[derive(Default)]
pub struct DummyReporter;

impl Reporter for DummyReporter {
    fn report_test_case_result(
        &self,
        _test_name: &QualifiedTestName,
        _result_kind: IndividualTestResultKind,
        _duration: Duration,
    ) {
    }
}

/// A reporter that outputs test results to stdout as they complete.
pub struct TestCaseReporter {
    printer: Printer,
}

impl TestCaseReporter {
    pub fn new(printer: Printer) -> Self {
        Self { printer }
    }
}

impl Reporter for TestCaseReporter {
    fn report_test_case_result(
        &self,
        test_name: &QualifiedTestName,
        result_kind: IndividualTestResultKind,
        duration: Duration,
    ) {
        if !show_for_status_level(self.printer.status_level(), &result_kind) {
            return;
        }

        let label = ResultLabel::from(&result_kind);
        let padding = label_padding(label.text().len());
        let colored_label = label.colored();
        let duration_str = format_duration_bracketed(duration);
        let test_path = format_test_path(test_name);

        let suffix = match &result_kind {
            IndividualTestResultKind::Skipped {
                reason: Some(reason),
            } => format!(": {reason}"),
            _ => String::new(),
        };

        if let Err(err) = write_test_result_line(
            self.printer,
            format!("{padding}{colored_label} {duration_str} {test_path}{suffix}"),
        ) {
            tracing::warn!("failed to write test result line: {err}");
        }
    }

    fn report_test_slow(&self, test_name: &QualifiedTestName, duration: Duration) {
        if self.printer.status_level() < StatusLevel::Slow {
            return;
        }

        let label = ResultLabel::Slow;
        let padding = label_padding(label.text().len());
        let colored_label = label.colored();
        let duration_str = format_duration_bracketed(duration);
        let test_path = format_test_path(test_name);

        if let Err(err) = write_test_result_line(
            self.printer,
            format!("{padding}{colored_label} {duration_str} {test_path}"),
        ) {
            tracing::warn!("failed to write slow test line: {err}");
        }
    }

    fn report_test_attempt(
        &self,
        test_name: &QualifiedTestName,
        attempt: u32,
        result_kind: IndividualTestResultKind,
        duration: Duration,
    ) {
        if self.printer.status_level() < StatusLevel::Retry {
            return;
        }

        // Skips don't go through the retry loop; we still render them so the
        // From impl and trait remain total.
        let label = ResultLabel::from(&result_kind);
        let label_len = "TRY ".len() + count_digits(attempt) + 1 + label.text().len();
        let padding = label_padding(label_len);
        let colored_status = label.colored();
        let duration_str = format_duration_bracketed(duration);
        let test_path = format_test_path(test_name);

        if let Err(err) = write_test_result_line(
            self.printer,
            format!("{padding}TRY {attempt} {colored_status} {duration_str} {test_path}"),
        ) {
            tracing::warn!("failed to write test attempt line: {err}");
        }
    }
}

/// The width that result labels (`PASS`, `FAIL`, `SKIP`, `SLOW`, `TRY N PASS`,
/// etc.) are right-padded to so columns align.
const LABEL_COLUMN_WIDTH: usize = 12;

fn label_padding(label_len: usize) -> String {
    " ".repeat(LABEL_COLUMN_WIDTH.saturating_sub(label_len))
}

fn write_test_result_line(printer: Printer, mut line: String) -> std::io::Result<()> {
    line.push('\n');
    let mut stdout = printer.stream_for_test_result().lock();
    stdout.write_all(line.as_bytes())
}

/// Render the colored `module::function[params]` portion of a result line.
fn format_test_path(test_name: &QualifiedTestName) -> String {
    let module = test_name.function_name().module_path().module_name().cyan();
    let fn_name = test_name.function_name().function_name().blue().bold();
    let params = test_name
        .parameters()
        .map(|parameters| format!("({parameters})").blue().bold().to_string())
        .unwrap_or_default();
    format!("{module}::{fn_name}{params}")
}

fn count_digits(n: u32) -> usize {
    n.checked_ilog10().unwrap_or(0) as usize + 1
}

#[derive(Clone, Copy)]
enum ResultLabel {
    Pass,
    Fail,
    Error,
    Skip,
    Slow,
}

impl ResultLabel {
    fn text(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Error => "ERROR",
            Self::Skip => "SKIP",
            Self::Slow => "SLOW",
        }
    }

    fn colored(self) -> String {
        let text = self.text();
        match self {
            Self::Pass => text.green().bold().to_string(),
            Self::Fail | Self::Error => text.red().bold().to_string(),
            Self::Skip | Self::Slow => text.yellow().bold().to_string(),
        }
    }
}

impl From<&IndividualTestResultKind> for ResultLabel {
    fn from(kind: &IndividualTestResultKind) -> Self {
        match kind {
            IndividualTestResultKind::Passed => Self::Pass,
            IndividualTestResultKind::Failed => Self::Fail,
            IndividualTestResultKind::Error => Self::Error,
            IndividualTestResultKind::Skipped { .. } => Self::Skip,
        }
    }
}
