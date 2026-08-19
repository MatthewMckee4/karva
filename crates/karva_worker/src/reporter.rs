use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Result, anyhow};
use camino::Utf8PathBuf;
use karva_diagnostic::{
    DisplayDiagnosticConfig, IndividualTestResultKind, Reporter, TestCaseReporter,
    TestExecutionResult,
};
use karva_ipc::{WorkerClient, WorkerEvent};
use karva_python_semantic::{QualifiedTestName, TestCacheKey};

/// Streams worker lifecycle and results while preserving terminal output.
pub struct WorkerReporter {
    /// Terminal-facing status reporter local to the worker process.
    output: TestCaseReporter,

    /// Shared transport for lifecycle and result frames.
    client: WorkerClient,

    /// Project directory used to render portable diagnostic paths.
    cwd: Utf8PathBuf,

    /// Formatting policy applied before results cross the process boundary.
    diagnostic_config: DisplayDiagnosticConfig,

    /// First transport failure retained for worker shutdown.
    send_error: Mutex<Option<String>>,
}

impl WorkerReporter {
    pub fn new(
        output: TestCaseReporter,
        client: WorkerClient,
        cwd: Utf8PathBuf,
        diagnostic_config: DisplayDiagnosticConfig,
    ) -> Self {
        Self {
            output,
            client,
            cwd,
            diagnostic_config,
            send_error: Mutex::new(None),
        }
    }

    /// Returns first transport error observed by reporter callbacks.
    pub fn finish(&self) -> Result<()> {
        let error = self
            .send_error
            .lock()
            .map_err(|_| anyhow!("Karva worker reporter lock poisoned"))?;
        if let Some(error) = error.as_ref() {
            anyhow::bail!("failed to stream worker event: {error}");
        }
        Ok(())
    }

    fn send(&self, event: WorkerEvent) {
        self.record_send_result(self.client.send_event(event));
    }

    fn record_send_result(&self, result: Result<()>) {
        if let Err(error) = result {
            tracing::warn!("failed to stream worker event: {error:#}");
            if let Ok(mut send_error) = self.send_error.lock()
                && send_error.is_none()
            {
                *send_error = Some(format!("{error:#}"));
            }
        }
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
        self.send(WorkerEvent::TestSlow);
    }

    fn report_test_started(&self, test_name: &QualifiedTestName) {
        self.record_send_result(self.client.checkpoint(test_name));
    }

    fn report_test_identified(&self, test_name: &QualifiedTestName) {
        self.report_test_started(test_name);
    }

    fn report_test_completed(&self, cache_key: &TestCacheKey, result: TestExecutionResult) {
        let result = result.render(&self.cwd, self.diagnostic_config);
        self.record_send_result(self.client.send_test_finished(cache_key, &result));
    }

    fn flush_test_results(&self) {
        self.record_send_result(self.client.flush());
    }
}
