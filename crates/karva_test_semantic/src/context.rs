use std::cell::RefCell;

use camino::Utf8Path;
use karva_collector::CollectionSettings;
use karva_diagnostic::{
    CapturedTestOutput, IndividualTestResultKind, Reporter, TestCaseRetry, TestExecutionAttempt,
    TestExecutionOutcome, TestRunResult,
};
use karva_metadata::ProjectSettings;
use karva_python_semantic::{ModulePath, QualifiedFunctionName, QualifiedTestName};
use ruff_db::diagnostic::Diagnostic;
use ruff_python_ast::PythonVersion;

/// Central context object that holds shared state for a test run.
///
/// Provides access to system operations, project settings, and test result
/// accumulation throughout the test discovery and execution phases.
pub struct Context<'a> {
    /// Current working directory.
    cwd: &'a Utf8Path,

    /// Project-level configuration settings.
    settings: &'a ProjectSettings,

    /// The Python version being used for this test run.
    python_version: PythonVersion,

    /// Accumulated test results, wrapped in `RefCell` for interior mutability.
    result: RefCell<TestRunResult>,

    /// Reporter for outputting test progress and results.
    reporter: &'a dyn Reporter,
}

impl<'a> Context<'a> {
    pub(crate) fn new(
        cwd: &'a Utf8Path,
        settings: &'a ProjectSettings,
        python_version: PythonVersion,
        reporter: &'a dyn Reporter,
    ) -> Self {
        Self {
            cwd,
            settings,
            python_version,
            result: RefCell::new(TestRunResult::default()),
            reporter,
        }
    }

    pub(crate) fn cwd(&self) -> &'a Utf8Path {
        self.cwd
    }

    pub(crate) fn settings(&self) -> &'a ProjectSettings {
        self.settings
    }

    pub(crate) fn collection_settings(&'a self) -> CollectionSettings<'a> {
        CollectionSettings {
            python_version: self.python_version,
            test_function_prefix: &self.settings.test().test_function_prefix,
            respect_ignore_files: self.settings.src().respect_ignore_files,
            collect_fixtures: true,
        }
    }

    pub(crate) fn result(&self) -> std::cell::RefMut<'_, TestRunResult> {
        self.result.borrow_mut()
    }

    pub(crate) fn into_result(self) -> TestRunResult {
        self.result.into_inner().into_sorted()
    }

    /// Record the start of a test execution. Forwarded to the reporter
    /// so cancellation logic can render per-test `SIGINT` lines naming
    /// the in-flight test.
    pub fn report_test_started(&self, test_case_name: &QualifiedTestName) {
        self.reporter.report_test_started(test_case_name);
    }

    /// Pair to [`Self::report_test_started`]: clears the in-flight marker
    /// once the test completes (passed, failed, or skipped).
    pub fn report_test_finished(&self, test_case_name: &QualifiedTestName) {
        self.reporter.report_test_finished(test_case_name);
    }

    pub fn register_test_case_result(
        &self,
        test_case_name: &QualifiedTestName,
        outcome: TestExecutionOutcome,
        duration: std::time::Duration,
        captured_output: Option<CapturedTestOutput>,
    ) -> bool {
        let passed = !outcome.is_non_success();

        self.result().register_test_case_result(
            test_case_name,
            outcome,
            duration,
            captured_output,
            Some(self.reporter),
        );

        passed
    }

    pub(crate) fn register_module_skip(&self, module_path: &ModulePath, reason: Option<String>) {
        let name = QualifiedTestName::new(
            QualifiedFunctionName::new("<module>".to_string(), module_path.clone()),
            None,
        );
        self.register_test_case_result(
            &name,
            TestExecutionOutcome::Skipped { reason },
            std::time::Duration::ZERO,
            None,
        );
    }

    /// Forward a per-attempt outcome to the reporter. Does not touch
    /// summary stats; the test's final outcome is registered separately
    /// via [`Self::register_retried_result`].
    pub fn report_test_attempt(
        &self,
        test_case_name: &QualifiedTestName,
        attempt: u32,
        result: IndividualTestResultKind,
        duration: std::time::Duration,
    ) {
        self.result().report_test_attempt(
            test_case_name,
            attempt,
            result,
            duration,
            Some(self.reporter),
        );
    }

    /// Mark a test as slow: increments the slow counter and emits the
    /// `SLOW` reporter line. Called once per test variant whose total
    /// runtime exceeded the configured `slow-timeout`.
    pub fn register_slow_test(
        &self,
        test_case_name: &QualifiedTestName,
        duration: std::time::Duration,
    ) {
        self.result()
            .register_slow_test(test_case_name, duration, Some(self.reporter));
    }

    /// Register the final outcome of a retried test. Updates summary stats
    /// (counting the test as flaky if it ultimately passed) without
    /// emitting a duplicate result line — the per-attempt `TRY N STATUS`
    /// lines already showed every attempt.
    pub fn register_retried_result(
        &self,
        test_case_name: &QualifiedTestName,
        outcome: TestExecutionOutcome,
        duration: std::time::Duration,
        retry: TestCaseRetry,
        captured_output: Option<CapturedTestOutput>,
        attempts: Vec<TestExecutionAttempt>,
    ) -> bool {
        let passed = !outcome.is_non_success();
        self.result().register_retried_result(
            test_case_name,
            outcome,
            duration,
            retry,
            captured_output,
            attempts,
        );
        passed
    }

    pub(crate) fn add_run_diagnostic(&self, diagnostic: Diagnostic) {
        self.result().add_run_diagnostic(diagnostic);
    }

    pub fn python_version(&self) -> PythonVersion {
        self.python_version
    }
}
