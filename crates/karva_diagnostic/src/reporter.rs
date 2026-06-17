use std::io::{Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use camino::Utf8Path;
use colored::Colorize;
use fs_err::{File, OpenOptions};
use karva_logging::time::format_duration_bracketed;
use karva_logging::{Printer, StatusLevel};
use karva_python_semantic::QualifiedTestName;
use serde::Serialize;

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
    progress_file: Option<Mutex<ProgressFile>>,
}

impl TestCaseReporter {
    pub fn new(printer: Printer) -> Self {
        Self {
            printer,
            progress_file: None,
        }
    }

    /// Direct the reporter to publish the currently running test's name and
    /// start time to `path` while it is in flight.
    pub fn with_progress_file(mut self, path: &Utf8Path) -> std::io::Result<Self> {
        self.progress_file = Some(Mutex::new(ProgressFile::spawn(path)?));
        Ok(self)
    }
}

struct ProgressFile {
    state: Arc<Mutex<Option<ProgressSnapshot>>>,
    stop: Arc<AtomicBool>,
    flusher: Option<JoinHandle<()>>,
}

#[derive(Clone, PartialEq, Eq, Serialize)]
struct ProgressSnapshot {
    name: String,
    start_unix_ms: u64,
}

impl ProgressFile {
    fn spawn(path: &Utf8Path) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?;
        let state = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let flusher_state = Arc::clone(&state);
        let flusher_stop = Arc::clone(&stop);
        let flusher = thread::spawn(move || {
            flush_progress_file(file, &flusher_state, &flusher_stop);
        });

        Ok(Self {
            state,
            stop,
            flusher: Some(flusher),
        })
    }

    fn set_current_test(&self, snapshot: ProgressSnapshot) -> std::io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("test progress state lock poisoned"))?;
        *state = Some(snapshot);
        Ok(())
    }

    fn clear(&self) -> std::io::Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| std::io::Error::other("test progress state lock poisoned"))?;
        *state = None;
        Ok(())
    }
}

impl Drop for ProgressFile {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(flusher) = self.flusher.take()
            && let Err(err) = flusher.join()
        {
            tracing::warn!(?err, "test progress flusher thread panicked");
        }
    }
}

const PROGRESS_FLUSH_INTERVAL: Duration = Duration::from_millis(25);

fn flush_progress_file(mut file: File, state: &Mutex<Option<ProgressSnapshot>>, stop: &AtomicBool) {
    let mut written = None;
    while !stop.load(Ordering::Acquire) {
        flush_progress_snapshot(&mut file, state, &mut written);
        thread::sleep(PROGRESS_FLUSH_INTERVAL);
    }
    flush_progress_snapshot(&mut file, state, &mut written);
    if let Err(err) = clear_progress_file(&mut file) {
        tracing::warn!("failed to clear test progress file: {err}");
    }
}

fn flush_progress_snapshot(
    file: &mut File,
    state: &Mutex<Option<ProgressSnapshot>>,
    written: &mut Option<ProgressSnapshot>,
) {
    let snapshot = if let Ok(state) = state.lock() {
        state.clone()
    } else {
        tracing::warn!("failed to lock test progress state");
        return;
    };

    if snapshot == *written {
        return;
    }

    let result = match snapshot.as_ref() {
        Some(snapshot) => write_progress_file(file, snapshot),
        None => clear_progress_file(file),
    };
    if let Err(err) = result {
        tracing::warn!("failed to flush test progress file: {err}");
        return;
    }

    *written = snapshot;
}

fn write_progress_file(file: &mut File, snapshot: &ProgressSnapshot) -> std::io::Result<()> {
    let body = serde_json::to_vec(snapshot).map_err(std::io::Error::other)?;
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&body)?;
    file.flush()?;
    Ok(())
}

fn clear_progress_file(file: &mut File) -> std::io::Result<()> {
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.flush()?;
    Ok(())
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

    fn report_test_started(&self, test_name: &QualifiedTestName) {
        let Some(progress_file) = self.progress_file.as_ref() else {
            return;
        };
        let start_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0);
        let snapshot = ProgressSnapshot {
            name: test_name.to_string(),
            start_unix_ms,
        };
        let Ok(progress_file) = progress_file.lock() else {
            tracing::warn!("failed to lock test progress file");
            return;
        };
        if let Err(err) = progress_file.set_current_test(snapshot) {
            tracing::warn!("failed to update test progress state: {err}");
        }
    }

    fn report_test_finished(&self, _test_name: &QualifiedTestName) {
        let Some(progress_file) = self.progress_file.as_ref() else {
            return;
        };
        let Ok(progress_file) = progress_file.lock() else {
            tracing::warn!("failed to lock test progress file");
            return;
        };
        if let Err(err) = progress_file.clear() {
            tracing::warn!("failed to clear test progress state: {err}");
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use camino::Utf8PathBuf;
    use karva_logging::Printer;
    use karva_python_semantic::{ModulePath, QualifiedFunctionName};

    use super::*;

    fn qualified_test_name() -> QualifiedTestName {
        QualifiedTestName::new(
            QualifiedFunctionName::new(
                "test_example".to_string(),
                ModulePath::new_with_name("test_module.py", "test_module".to_string()),
            ),
            None,
        )
    }

    fn wait_for_progress_snapshot(
        path: &Utf8Path,
        matches: impl Fn(&serde_json::Value) -> bool,
    ) -> serde_json::Value {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(1) {
            let body = std::fs::read_to_string(path).expect("progress file should exist");
            if let Ok(progress) = serde_json::from_str(&body)
                && matches(&progress)
            {
                return progress;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for progress file snapshot");
    }

    fn wait_for_empty_progress_body(path: &Utf8Path) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(1) {
            let body = std::fs::read_to_string(path).expect("progress file should exist");
            if body.is_empty() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for progress file to clear");
    }

    #[test]
    fn progress_file_is_written_and_cleared() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let path = Utf8PathBuf::try_from(temp_dir.path().join("current-test.json"))
            .expect("temp path should be UTF-8");
        let reporter = TestCaseReporter::new(Printer::default())
            .with_progress_file(&path)
            .expect("progress file should open");
        let test_name = qualified_test_name();

        reporter.report_test_started(&test_name);

        let progress = wait_for_progress_snapshot(&path, |_| true);
        assert_eq!(progress["name"], "test_module::test_example");
        assert!(progress["start_unix_ms"].as_u64().is_some());

        reporter.report_test_finished(&test_name);

        wait_for_empty_progress_body(&path);
    }

    #[test]
    fn progress_file_reuses_marker_without_stale_content() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let path = Utf8PathBuf::try_from(temp_dir.path().join("current-test.json"))
            .expect("temp path should be UTF-8");
        let reporter = TestCaseReporter::new(Printer::default())
            .with_progress_file(&path)
            .expect("progress file should open");

        reporter.report_test_started(&QualifiedTestName::new(
            QualifiedFunctionName::new(
                "test_example_with_a_much_longer_name".to_string(),
                ModulePath::new_with_name("test_module.py", "test_module".to_string()),
            ),
            None,
        ));
        let progress = wait_for_progress_snapshot(&path, |progress| {
            progress["name"] == "test_module::test_example_with_a_much_longer_name"
        });
        assert_eq!(
            progress["name"],
            "test_module::test_example_with_a_much_longer_name"
        );

        reporter.report_test_finished(&qualified_test_name());
        reporter.report_test_started(&qualified_test_name());

        let progress = wait_for_progress_snapshot(&path, |progress| {
            progress["name"] == "test_module::test_example"
        });
        assert_eq!(progress["name"], "test_module::test_example");
    }

    #[test]
    fn progress_file_open_error_includes_path() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let path = Utf8PathBuf::try_from(
            temp_dir
                .path()
                .join("missing-parent")
                .join("current-test.json"),
        )
        .expect("temp path should be UTF-8");

        let Err(error) = TestCaseReporter::new(Printer::default()).with_progress_file(&path) else {
            panic!("missing parent should fail");
        };

        assert!(
            error.to_string().contains(path.as_str()),
            "unexpected error: {error}"
        );
    }
}
