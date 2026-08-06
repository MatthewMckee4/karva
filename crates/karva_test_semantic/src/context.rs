use std::collections::BTreeSet;

use camino::Utf8Path;
use karva_collector::CollectionSettings;
use karva_diagnostic::{
    CapturedTestOutput, Diagnostic, IndividualTestResultKind, Reporter, TestExecutionOutcome,
    TestExecutionResult, sort_diagnostics_for_display,
};
use karva_metadata::ProjectSettings;
use karva_python_semantic::{ModulePath, QualifiedFunctionName, QualifiedTestName, TestCacheKey};
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

    /// Cases already committed by an earlier worker generation.
    resume_skip: &'a BTreeSet<TestCacheKey>,

    /// Whether diagnostics should include the full Python call chain.
    verbose: bool,
}

/// Run-level diagnostics retained until execution completes.
#[derive(Default)]
pub struct RunState {
    run_diagnostics: Vec<Diagnostic>,
}

impl<'a> Context<'a> {
    pub(super) fn new(
        cwd: &'a Utf8Path,
        settings: &'a ProjectSettings,
        python_version: PythonVersion,
        reporter: &'a dyn Reporter,
        resume_skip: &'a BTreeSet<TestCacheKey>,
        verbose: bool,
    ) -> Self {
        Self {
            cwd,
            settings,
            python_version,
            reporter,
            resume_skip,
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

    /// Whether crash recovery already committed this exact case.
    pub(super) fn should_resume_skip(&self, cache_key: &TestCacheKey) -> bool {
        self.resume_skip.contains(cache_key)
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

    /// Refines the active test name after fixture-derived parameters resolve.
    pub fn report_test_identified(&self, test_case_name: &QualifiedTestName) {
        self.reporter.report_test_identified(test_case_name);
    }

    /// Returns the parser target matching the embedded interpreter.
    pub fn python_version(&self) -> PythonVersion {
        self.python_version
    }
}

impl RunState {
    pub(super) fn into_result(mut self) -> Vec<Diagnostic> {
        sort_diagnostics_for_display(&mut self.run_diagnostics);
        self.run_diagnostics
    }

    pub(super) fn add_run_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.run_diagnostics.push(diagnostic);
    }
}

impl Context<'_> {
    /// Reports a final non-retried test outcome.
    ///
    /// Returns whether the outcome counts as successful for failure-budget accounting.
    pub fn register_test_case_result(
        &self,
        test_case_name: &QualifiedTestName,
        outcome: TestExecutionOutcome,
        duration: std::time::Duration,
        captured_output: Option<CapturedTestOutput>,
    ) -> bool {
        let passed = !outcome.is_non_success();

        let cache_key = test_case_name.cache_key();
        let test_case =
            TestExecutionResult::new(test_case_name, outcome, duration, captured_output);
        self.reporter.report_test_case_result(
            test_case_name,
            test_case.outcome().result_kind(),
            duration,
        );
        self.reporter.report_test_completed(&cache_key, test_case);

        passed
    }

    pub(super) fn register_module_skip(&self, module_path: &ModulePath, reason: Option<String>) {
        let name = QualifiedTestName::new(QualifiedFunctionName::new(
            "<module>".to_string(),
            module_path.clone(),
        ));
        self.register_test_case_result(
            &name,
            TestExecutionOutcome::Skipped { reason },
            std::time::Duration::ZERO,
            None,
        );
    }

    /// Forwards a per-attempt outcome to the reporter.
    pub fn report_test_attempt(
        &self,
        test_case_name: &QualifiedTestName,
        attempt: u32,
        result: IndividualTestResultKind,
        duration: std::time::Duration,
    ) {
        self.reporter
            .report_test_attempt(test_case_name, attempt, result, duration);
    }

    /// Mark a test as slow: increments the slow counter and emits the
    /// `SLOW` reporter line. Called once per test variant whose total
    /// runtime exceeded the configured `slow-timeout`.
    pub fn register_slow_test(
        &self,
        test_case_name: &QualifiedTestName,
        duration: std::time::Duration,
    ) {
        self.reporter.report_test_slow(test_case_name, duration);
    }

    /// Reports the final outcome of a retried test without emitting a duplicate
    /// result line; per-attempt `TRY N STATUS` lines already showed each attempt.
    pub fn register_retried_result(
        &self,
        test_case_name: &QualifiedTestName,
        test_case: TestExecutionResult,
    ) -> bool {
        let passed = !test_case.outcome().is_non_success() && !test_case.is_flaky_failure();
        let cache_key = test_case_name.cache_key();
        self.reporter.report_test_completed(&cache_key, test_case);
        passed
    }
}
