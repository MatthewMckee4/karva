use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Result, anyhow};
use camino::Utf8PathBuf;
use karva_diagnostic::{
    Diagnostic, DisplayDiagnosticConfig, IndividualTestResultKind, Reporter, TestCaseReporter,
    TestExecutionResult, render_diagnostic,
};
use karva_ipc::{WorkerClient, WorkerEvent};
use karva_python_semantic::{QualifiedTestName, TestCacheKey};

/// Streams worker lifecycle and results while preserving terminal output.
pub struct WorkerReporter {
    output: TestCaseReporter,
    client: WorkerClient,
    cwd: Utf8PathBuf,
    diagnostic_config: DisplayDiagnosticConfig,
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
        if let Err(error) = self.client.send_event(event) {
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
        self.send(WorkerEvent::TestStarted {
            name: test_name.to_string(),
        });
    }

    fn report_test_completed(&self, cache_key: &TestCacheKey, result: &TestExecutionResult) {
        self.send(WorkerEvent::TestFinished {
            cache_key: cache_key.clone(),
            result: result.clone().render(&self.cwd, self.diagnostic_config),
        });
    }

    fn report_run_diagnostic(&self, diagnostic: &Diagnostic) {
        self.send(WorkerEvent::RunDiagnostic(render_diagnostic(
            diagnostic,
            &self.cwd,
            self.diagnostic_config,
        )));
    }
}
