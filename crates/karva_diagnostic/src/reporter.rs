use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

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

    /// Notify that a test has fully completed for accounting purposes.
    ///
    /// Called exactly once per test (after every attempt has run for retried
    /// tests, after the single attempt for non-retried tests). Reporters
    /// that drive a progress display use this hook so the count advances
    /// once per test rather than once per attempt. Default no-op.
    fn notify_test_completed(&self, test_name: &QualifiedTestName) {
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

/// Sink for preformatted reporter result lines.
///
/// Each `write_line` call must emit exactly one line (a `\n` is appended by
/// the implementation). Lines from one sink are serialized; the orchestrator
/// merges across sinks. Used to keep worker output from interleaving on
/// stdout — workers write to a [`FileLineSink`] backed by a per-worker file
/// and the orchestrator drains those files line by line.
pub trait LineSink: Send + Sync {
    fn write_line(&self, line: &str);
}

/// Writes lines straight to the process stdout (locked per call).
///
/// Suitable when the reporter runs in the same process that owns stdout.
/// Cross-process workers should use [`FileLineSink`] instead — multiple
/// processes locking stdout independently does not actually serialize their
/// writes.
pub struct StdoutLineSink;

impl LineSink for StdoutLineSink {
    fn write_line(&self, line: &str) {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
    }
}

/// Appends lines to a file opened with `O_APPEND`.
///
/// A `Mutex` guards against intra-process races (the reporter is shared
/// across worker threads). Each `write_line` issues a single `writeln!` so
/// the orchestrator's drain — which reads from the file and splits on `\n` —
/// either sees a complete line or buffers a trailing partial line for the
/// next read.
pub struct FileLineSink {
    file: Mutex<File>,
}

impl FileLineSink {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }
}

impl LineSink for FileLineSink {
    fn write_line(&self, line: &str) {
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(file, "{line}");
        }
    }
}

/// A reporter that emits one line per result to a [`LineSink`].
pub struct TestCaseReporter {
    printer: Printer,
    sink: Box<dyn LineSink>,
}

impl TestCaseReporter {
    pub fn new(printer: Printer, sink: Box<dyn LineSink>) -> Self {
        Self { printer, sink }
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

        self.sink.write_line(&format!(
            "{padding}{colored_label} {duration_str} {test_path}{suffix}"
        ));
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

        self.sink.write_line(&format!(
            "{padding}{colored_label} {duration_str} {test_path}"
        ));
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

        self.sink.write_line(&format!(
            "{padding}TRY {attempt} {colored_status} {duration_str} {test_path}"
        ));
    }
}

/// Wraps another reporter and appends one byte to a file each time a test
/// completes (counted once per test, regardless of retries).
///
/// The orchestrator polls the file's length to drive a live progress
/// display without coordinating with workers via a richer protocol. The
/// append uses `O_APPEND`, which is atomic on POSIX, so the orchestrator
/// can read the length concurrently without locking.
pub struct ProgressTrackingReporter<R: Reporter> {
    inner: R,
    progress_file: PathBuf,
}

impl<R: Reporter> ProgressTrackingReporter<R> {
    pub fn new(inner: R, progress_file: PathBuf) -> Self {
        Self {
            inner,
            progress_file,
        }
    }

    fn append_tick(&self) {
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.progress_file)
        {
            let _ = file.write_all(b"\x01");
        }
    }
}

impl<R: Reporter> Reporter for ProgressTrackingReporter<R> {
    fn report_test_case_result(
        &self,
        test_name: &QualifiedTestName,
        result_kind: IndividualTestResultKind,
        duration: Duration,
    ) {
        self.inner
            .report_test_case_result(test_name, result_kind, duration);
    }

    fn report_test_attempt(
        &self,
        test_name: &QualifiedTestName,
        attempt: u32,
        result_kind: IndividualTestResultKind,
        duration: Duration,
    ) {
        self.inner
            .report_test_attempt(test_name, attempt, result_kind, duration);
    }

    fn report_test_slow(&self, test_name: &QualifiedTestName, duration: Duration) {
        self.inner.report_test_slow(test_name, duration);
    }

    fn notify_test_completed(&self, test_name: &QualifiedTestName) {
        self.inner.notify_test_completed(test_name);
        self.append_tick();
    }
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
