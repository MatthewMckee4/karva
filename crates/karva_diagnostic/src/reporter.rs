use std::fmt::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use camino::Utf8PathBuf;
use colored::Colorize;
use karva_logging::time::format_duration_bracketed;
use karva_logging::{Printer, StatusLevel};
use karva_python_semantic::QualifiedTestName;

use crate::result::IndividualTestResultKind;

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

    /// Called when a test finishes (passed, failed, or skipped) so the
    /// reporter can clear any in-flight state recorded by
    /// [`Self::report_test_started`]. Default no-op.
    fn report_test_finished(&self, test_name: &QualifiedTestName) {
        let _ = test_name;
    }
}

fn show_for_status_level(level: StatusLevel, kind: &IndividualTestResultKind) -> bool {
    // Levels are cumulative, like nextest: each level shows itself plus all
    // earlier levels. The `Slow` line is gated separately in
    // `report_test_slow`, so `Slow` here acts the same as `Retry`.
    match level {
        StatusLevel::None => false,
        StatusLevel::Fail | StatusLevel::Retry | StatusLevel::Slow => {
            matches!(kind, IndividualTestResultKind::Failed)
        }
        StatusLevel::Pass => matches!(
            kind,
            IndividualTestResultKind::Failed | IndividualTestResultKind::Passed
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
    /// Optional path to a JSON file describing the test currently
    /// executing. The orchestrator reads this on Ctrl+C to render
    /// per-test `SIGINT` lines.
    progress_file: Option<Utf8PathBuf>,
}

impl TestCaseReporter {
    pub fn new(printer: Printer) -> Self {
        Self {
            printer,
            progress_file: None,
        }
    }

    /// Direct the reporter to write the currently running test's name and
    /// start time to `path` whenever a test begins, and remove the file
    /// when it ends.
    #[must_use]
    pub fn with_progress_file(mut self, path: Utf8PathBuf) -> Self {
        self.progress_file = Some(path);
        self
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

        let mut stdout = self.printer.stream_for_test_result().lock();
        writeln!(
            stdout,
            "{padding}{colored_label} {duration_str} {test_path}{suffix}"
        )
        .ok();
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

        let mut stdout = self.printer.stream_for_test_result().lock();
        writeln!(
            stdout,
            "{padding}{colored_label} {duration_str} {test_path}"
        )
        .ok();
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

        let mut stdout = self.printer.stream_for_test_result().lock();
        writeln!(
            stdout,
            "{padding}TRY {attempt} {colored_status} {duration_str} {test_path}"
        )
        .ok();
    }

    fn report_test_started(&self, test_name: &QualifiedTestName) {
        let Some(path) = self.progress_file.as_ref() else {
            return;
        };
        let start_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        // Avoid pulling in `karva_cache::CurrentTest` here (would be a
        // circular dep). The cache crate deserialises the same JSON shape.
        let body = format!(
            "{{\"name\":{},\"start_unix_ms\":{start_unix_ms}}}",
            json_string(&test_name.to_string()),
        );
        let _ = std::fs::write(path, body);
    }

    fn report_test_finished(&self, _test_name: &QualifiedTestName) {
        if let Some(path) = self.progress_file.as_ref() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Quote a string for JSON. Stays in this crate so we don't take a hard
/// dependency on `serde_json` just for one field.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The width that result labels (`PASS`, `FAIL`, `SKIP`, `SLOW`, `TRY N PASS`,
/// etc.) are right-padded to so columns align.
const LABEL_COLUMN_WIDTH: usize = 12;

fn label_padding(label_len: usize) -> String {
    " ".repeat(LABEL_COLUMN_WIDTH.saturating_sub(label_len))
}

/// Render the colored `module::function[params]` portion of a result line.
fn format_test_path(test_name: &QualifiedTestName) -> String {
    let module = test_name.function_name().module_path().module_name().cyan();
    let fn_name = test_name.function_name().function_name().blue().bold();
    let params = test_name
        .params()
        .map(|p| p.blue().bold().to_string())
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
    Skip,
    Slow,
}

impl ResultLabel {
    fn text(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Skip => "SKIP",
            Self::Slow => "SLOW",
        }
    }

    fn colored(self) -> String {
        let text = self.text();
        match self {
            Self::Pass => text.green().bold().to_string(),
            Self::Fail => text.red().bold().to_string(),
            Self::Skip | Self::Slow => text.yellow().bold().to_string(),
        }
    }
}

impl From<&IndividualTestResultKind> for ResultLabel {
    fn from(kind: &IndividualTestResultKind) -> Self {
        match kind {
            IndividualTestResultKind::Passed => Self::Pass,
            IndividualTestResultKind::Failed => Self::Fail,
            IndividualTestResultKind::Skipped { .. } => Self::Skip,
        }
    }
}
