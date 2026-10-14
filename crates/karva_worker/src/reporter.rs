use std::fs::File;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::fd::AsFd;
#[cfg(windows)]
use std::os::windows::io::AsHandle;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Result, anyhow};
use camino::Utf8PathBuf;
use karva_diagnostic::{
    CapturedOutputReader, CapturedTestOutput, DisplayDiagnosticConfig, IndividualTestResultKind,
    Reporter, TestCaseReporter, TestExecutionResult,
};
use karva_ipc::{WorkerClient, WorkerEvent};
use karva_python_semantic::{QualifiedTestName, TestCacheKey};

/// Streams worker lifecycle and results while preserving terminal output.
pub struct WorkerReporter {
    /// Terminal-facing status reporter local to the worker process.
    output: TestCaseReporter,

    /// Private output route reused across attempts, absent for protocol-only clients.
    output_reader: Option<Arc<WorkerOutputReader>>,

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
        output_marker: Option<[u8; 16]>,
        client: WorkerClient,
        cwd: Utf8PathBuf,
        diagnostic_config: DisplayDiagnosticConfig,
    ) -> io::Result<Self> {
        let output_reader = output_marker
            .map(|marker| WorkerOutputReader::new(client.clone(), marker).map(Arc::new))
            .transpose()?;
        Ok(Self {
            output,
            output_reader,
            client,
            cwd,
            diagnostic_config,
            send_error: Mutex::new(None),
        })
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

/// Bridges semantic capture to controller storage without linking the binaries.
struct WorkerOutputReader {
    client: WorkerClient,
    marker: [u8; 16],

    /// Worker-lifetime routes remain valid if test code closes descriptors 1 or 2.
    stdout: File,

    /// Separate inherited stderr route for ordered capture boundaries.
    stderr: File,

    /// Normal completion metadata; a retry emits the preceding window before its begin marker.
    pending: Mutex<PendingCompletion>,
}

/// Capture and status metadata carried by an ordinary result rather than extra frames.
#[derive(Default)]
struct PendingCompletion {
    attempt: Option<u32>,
    status: bool,
}

impl WorkerOutputReader {
    fn take_completion(&self) -> Result<PendingCompletion, String> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "output completion lock poisoned")?;
        Ok(std::mem::take(&mut *pending))
    }

    fn new(client: WorkerClient, marker: [u8; 16]) -> io::Result<Self> {
        #[cfg(unix)]
        let (stdout, stderr) = (
            File::from(io::stdout().as_fd().try_clone_to_owned()?),
            File::from(io::stderr().as_fd().try_clone_to_owned()?),
        );
        #[cfg(windows)]
        let (stdout, stderr) = (
            File::from(io::stdout().as_handle().try_clone_to_owned()?),
            File::from(io::stderr().as_handle().try_clone_to_owned()?),
        );
        Ok(Self {
            client,
            marker,
            stdout,
            stderr,
            pending: Mutex::new(PendingCompletion::default()),
        })
    }
}

impl CapturedOutputReader for WorkerOutputReader {
    fn boundary(&self, tag: u8) -> Result<(), String> {
        if tag == b'B' {
            let previous = self
                .pending
                .lock()
                .map_err(|_| "output completion lock poisoned")?
                .attempt
                .take();
            if let Some(attempt) = previous {
                self.client
                    .defer_captured_output(attempt)
                    .map_err(|error| format!("{error:#}"))?;
            }
        }
        let mut frame = [0; 18];
        frame[1..17].copy_from_slice(&self.marker);
        frame[17] = tag;
        (&self.stdout)
            .write_all(&frame)
            .and_then(|()| (&self.stderr).write_all(&frame))
            .map_err(|error| error.to_string())
    }
    fn defer(&self, attempt: u32) -> Result<(), String> {
        self.pending
            .lock()
            .map_err(|_| "output completion lock poisoned")?
            .attempt = Some(attempt);
        Ok(())
    }
    fn read(&self) -> Result<CapturedTestOutput, String> {
        self.client
            .read_captured_output()
            .map_err(|error| format!("{error:#}"))
    }
}

impl Reporter for WorkerReporter {
    fn captured_output_reader(&self) -> Option<Arc<dyn CapturedOutputReader>> {
        self.output_reader
            .as_ref()
            .map(|reader| Arc::clone(reader) as Arc<dyn CapturedOutputReader>)
    }

    fn report_warning(&self, warning: karva_diagnostic::TestWarning) {
        self.send(WorkerEvent::Warning(warning));
    }

    fn report_test_case_result(
        &self,
        test_name: &QualifiedTestName,
        result_kind: IndividualTestResultKind,
        duration: Duration,
    ) {
        if let Some(reader) = &self.output_reader {
            match reader.pending.lock() {
                Ok(mut pending) => pending.status = true,
                Err(_) => self.record_send_result(Err(anyhow!("output completion lock poisoned"))),
            }
        } else {
            self.output
                .report_test_case_result(test_name, result_kind, duration);
        }
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
        let pending = match self
            .output_reader
            .as_ref()
            .map(|reader| reader.take_completion())
            .transpose()
        {
            Ok(pending) => pending.unwrap_or_default(),
            Err(error) => {
                self.record_send_result(Err(anyhow!(error)));
                PendingCompletion::default()
            }
        };
        self.record_send_result(self.client.send_test_finished(
            cache_key,
            &result,
            pending.attempt,
            pending.status,
        ));
    }

    fn report_test_timed_out(
        &self,
        test_name: &QualifiedTestName,
        result: TestExecutionResult,
        fail_on_flaky: bool,
        junit_fail_on_flaky: bool,
    ) {
        let attempt = result
            .retry()
            .map_or(1, karva_diagnostic::TestCaseRetry::attempts);
        let max_attempts = result
            .retry()
            .map_or(1, karva_diagnostic::TestCaseRetry::max_attempts);
        if max_attempts > 1 {
            self.output.report_test_attempt(
                test_name,
                attempt,
                IndividualTestResultKind::Failed,
                result.duration(),
            );
        } else {
            self.output.report_test_case_result(
                test_name,
                IndividualTestResultKind::Failed,
                result.duration(),
            );
        }
        self.send(WorkerEvent::TestTimedOut {
            cache_key: test_name.cache_key(),
            result: Box::new(result.render(&self.cwd, self.diagnostic_config)),
            fail_on_flaky,
            junit_fail_on_flaky,
        });
        self.record_send_result(self.client.flush());
    }

    fn flush_test_results(&self) {
        self.record_send_result(self.client.flush());
    }
}
