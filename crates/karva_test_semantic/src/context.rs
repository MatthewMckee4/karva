use camino::Utf8Path;
use karva_collector::CollectionSettings;
use karva_diagnostic::{
    CapturedTestOutput, Diagnostic, IndividualTestResultKind, Reporter, TestExecutionOutcome,
    TestExecutionResult, TestRunResult,
};
use karva_metadata::ProjectSettings;
use karva_python_semantic::{ModulePath, QualifiedFunctionName, QualifiedTestName};
use ruff_python_ast::PythonVersion;

/// Immutable configuration and reporting services shared by one test run.
pub struct Context<'a> {
    /// Current working directory.
    cwd: &'a Utf8Path,

    /// Project-level configuration settings.
    settings: &'a ProjectSettings,

    /// The Python version being used for this test run.
    python_version: PythonVersion,

    /// Reporter for outputting test progress and results.
    reporter: &'a dyn Reporter,

    /// Whether diagnostics should include the full Python call chain.
    verbose: bool,
}

/// Mutable result state owned by the active execution path.
#[derive(Default)]
pub struct RunState {
    result: TestRunResult,
}

impl<'a> Context<'a> {
    pub(super) fn new(
        cwd: &'a Utf8Path,
        settings: &'a ProjectSettings,
        python_version: PythonVersion,
        reporter: &'a dyn Reporter,
        verbose: bool,
    ) -> Self {
        Self {
            cwd,
            settings,
            python_version,
            reporter,
            verbose,
        }
    }

    pub(super) fn cwd(&self) -> &'a Utf8Path {
        self.cwd
    }

    pub(super) fn settings(&self) -> &'a ProjectSettings {
        self.settings
    }

    pub(super) fn is_verbose(&self) -> bool {
        self.verbose
    }

    pub(super) fn collection_settings(&'a self) -> CollectionSettings<'a> {
        CollectionSettings {
            python_version: self.python_version,
            test_function_prefix: &self.settings.test().test_function_prefix,
            respect_ignore_files: self.settings.src().respect_ignore_files,
            collect_fixtures: true,
        }
    }

    /// Record the start of a test execution. Forwarded to the reporter
    /// so cancellation logic can render per-test `SIGINT` lines naming
    /// the in-flight test.
    pub fn report_test_started(&self, test_case_name: &QualifiedTestName) {
        self.reporter.report_test_started(test_case_name);
    }

    /// Returns the parser target matching the embedded interpreter.
    pub fn python_version(&self) -> PythonVersion {
        self.python_version
    }
}

impl RunState {
    pub(super) fn into_result(self) -> TestRunResult {
        self.result.into_sorted()
    }

    /// Stores and reports a final non-retried test outcome.
    ///
    /// Returns whether the outcome counts as successful for failure-budget accounting.
    pub fn register_test_case_result(
        &mut self,
        context: &Context<'_>,
        test_case_name: &QualifiedTestName,
        outcome: TestExecutionOutcome,
        duration: std::time::Duration,
        captured_output: Option<CapturedTestOutput>,
    ) -> bool {
        let passed = !outcome.is_non_success();

        self.result.register_test_case_result(
            test_case_name,
            outcome,
            duration,
            captured_output,
            Some(context.reporter),
        );

        passed
    }

    pub(super) fn register_module_skip(
        &mut self,
        context: &Context<'_>,
        module_path: &ModulePath,
        reason: Option<String>,
    ) {
        let name = QualifiedTestName::new(QualifiedFunctionName::new(
            "<module>".to_string(),
            module_path.clone(),
        ));
        self.register_test_case_result(
            context,
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
        context: &Context<'_>,
        test_case_name: &QualifiedTestName,
        attempt: u32,
        result: IndividualTestResultKind,
        duration: std::time::Duration,
    ) {
        self.result.report_test_attempt(
            test_case_name,
            attempt,
            result,
            duration,
            Some(context.reporter),
        );
    }

    /// Mark a test as slow: increments the slow counter and emits the
    /// `SLOW` reporter line. Called once per test variant whose total
    /// runtime exceeded the configured `slow-timeout`.
    pub fn register_slow_test(
        &mut self,
        context: &Context<'_>,
        test_case_name: &QualifiedTestName,
        duration: std::time::Duration,
    ) {
        self.result
            .register_slow_test(test_case_name, duration, Some(context.reporter));
    }

    /// Register the final outcome of a retried test. Updates summary stats
    /// (counting the test as flaky if it ultimately passed) without
    /// emitting a duplicate result line — the per-attempt `TRY N STATUS`
    /// lines already showed every attempt.
    pub fn register_retried_result(
        &mut self,
        context: &Context<'_>,
        test_case_name: &QualifiedTestName,
        test_case: TestExecutionResult,
    ) -> bool {
        let passed = !test_case.outcome().is_non_success() && !test_case.is_flaky_failure();
        let cache_key = test_case_name.cache_key();
        self.result
            .register_retried_result(cache_key, test_case, Some(context.reporter));
        passed
    }

    pub(super) fn add_run_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.result.add_run_diagnostic(diagnostic);
    }
}
