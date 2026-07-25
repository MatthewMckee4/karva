mod case;
mod flaky;
mod kind;
mod output;
mod stats;

use std::collections::HashMap;

use karva_python_semantic::{QualifiedFunctionName, QualifiedTestName};
use ruff_db::diagnostic::Diagnostic;

use crate::reporter::Reporter;

pub use case::{
    DiagnosticSeverity, RenderedDiagnostic, TestCaseOutcome, TestCaseResult, TestCaseRetry,
    TestExecutionOutcome, TestExecutionResult,
};
pub use flaky::{DisplayFlakyTest, DisplayFlakyTests, FlakyTest};
pub use kind::{IndividualTestResultKind, TestResultKind};
pub use output::{CapturedTestOutcome, CapturedTestOutput};
pub use stats::TestResultStats;

/// Represents the result of a test run.
///
/// This is held in the test context and updated throughout the test run.
#[derive(Debug, Clone, Default)]
pub struct TestRunResult {
    /// Diagnostics generated during test discovery and collection,
    /// in the order they were emitted.
    run_diagnostics: Vec<Diagnostic>,

    /// Stats generated during test execution.
    stats: TestResultStats,

    /// The duration of each test function.
    durations: HashMap<QualifiedFunctionName, std::time::Duration>,

    /// Names of tests that failed during this run.
    failed_tests: Vec<QualifiedFunctionName>,

    /// Tests that passed only after at least one retry.
    flaky_tests: Vec<FlakyTest>,

    /// Final outcome for each executed test variant.
    test_cases: Vec<TestExecutionResult>,

    /// Captured Python stdout/stderr for individual test variants.
    captured_outputs: Vec<CapturedTestOutput>,
}

/// Orders diagnostics for display.
///
/// `Diagnostic::ruff_start_ordering` panics when either diagnostic has no
/// primary span pointing at a `SourceFile`, which is the case for karva
/// diagnostics like `failed-to-import-module` that describe a whole module
/// rather than a location within it. Diagnostics with a source file sort by
/// that ordering first; span-less diagnostics sort after them, by id and
/// then message, so they still appear rather than being dropped or causing
/// the sort to panic.
fn diagnostic_display_ordering(a: &Diagnostic, b: &Diagnostic) -> std::cmp::Ordering {
    match (a.ruff_source_file(), b.ruff_source_file()) {
        (Some(_), Some(_)) => a.ruff_start_ordering(b),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a
            .id()
            .cmp(&b.id())
            .then_with(|| a.primary_message().cmp(b.primary_message())),
    }
}

impl TestRunResult {
    pub fn run_diagnostics(&self) -> &[Diagnostic] {
        &self.run_diagnostics
    }

    pub fn add_run_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.run_diagnostics.push(diagnostic);
    }

    pub fn stats(&self) -> &TestResultStats {
        &self.stats
    }

    pub fn register_captured_output(
        &mut self,
        test_case_name: &QualifiedTestName,
        result: &IndividualTestResultKind,
        stdout: String,
        stderr: String,
    ) {
        let output = CapturedTestOutput::new(
            test_case_name.to_string(),
            CapturedTestOutcome::from(result),
            stdout,
            stderr,
        );
        if !output.is_empty() {
            self.captured_outputs.push(output);
        }
    }

    pub fn register_test_case_result(
        &mut self,
        test_case_name: &QualifiedTestName,
        outcome: TestExecutionOutcome,
        duration: std::time::Duration,
        reporter: Option<&dyn Reporter>,
    ) {
        let result = outcome.result_kind();
        self.stats.add(result.clone().into());

        let function_name = test_case_name.function_name().clone();
        self.test_cases
            .push(TestCaseResult::new(test_case_name, outcome, duration));

        if matches!(&result, IndividualTestResultKind::Failed) {
            self.failed_tests.push(function_name.clone());
        }

        if let Some(reporter) = reporter {
            reporter.report_test_case_result(test_case_name, result, duration);
        }

        self.durations
            .entry(function_name)
            .and_modify(|existing_duration| *existing_duration += duration)
            .or_insert(duration);
    }

    /// Register the final outcome of a test that went through retries.
    /// Updates summary stats and durations but does not emit a separate
    /// `report_test_case_result` line — the per-attempt `TRY N STATUS`
    /// lines are the user-visible output for a retried test.
    ///
    /// `passed_on` is the 1-indexed attempt number that ultimately succeeded
    /// (only meaningful when `result` is `Passed`). `total_attempts` mirrors
    /// nextest's `FLAKY M/T` denominator: the maximum number of attempts the
    /// test was allowed (`retries + 1`), not just the count that ran.
    /// When the final outcome is `Passed`, the test is counted as flaky.
    pub fn register_retried_result(
        &mut self,
        test_case_name: &QualifiedTestName,
        outcome: TestExecutionOutcome,
        duration: std::time::Duration,
        passed_on: u32,
        total_attempts: u32,
        _reporter: Option<&dyn Reporter>,
    ) {
        let result = outcome.result_kind();
        self.stats.add(result.clone().into());

        let function_name = test_case_name.function_name().clone();
        self.test_cases.push(TestCaseResult::retried(
            test_case_name,
            outcome,
            duration,
            TestCaseRetry::new(passed_on, total_attempts),
        ));

        if matches!(&result, IndividualTestResultKind::Failed) {
            self.failed_tests.push(function_name.clone());
        } else if matches!(&result, IndividualTestResultKind::Passed) {
            self.stats.add(TestResultKind::Flaky);
            self.flaky_tests.push(FlakyTest::from_qualified_name(
                test_case_name,
                passed_on,
                total_attempts,
                duration,
            ));
        }

        self.durations
            .entry(function_name)
            .and_modify(|existing_duration| *existing_duration += duration)
            .or_insert(duration);
    }

    /// Forward a per-attempt notification to the reporter without touching
    /// summary stats. Called once per attempt of a retried test, including
    /// the final attempt.
    pub fn report_test_attempt(
        &self,
        test_case_name: &QualifiedTestName,
        attempt: u32,
        result: IndividualTestResultKind,
        duration: std::time::Duration,
        reporter: Option<&dyn Reporter>,
    ) {
        if let Some(reporter) = reporter {
            reporter.report_test_attempt(test_case_name, attempt, result, duration);
        }
    }

    /// Mark a test as slow: increments the slow counter and emits a `SLOW`
    /// line through the reporter. The test's actual outcome (pass/fail) is
    /// registered separately.
    pub fn register_slow_test(
        &mut self,
        test_case_name: &QualifiedTestName,
        duration: std::time::Duration,
        reporter: Option<&dyn Reporter>,
    ) {
        self.stats.add(TestResultKind::Slow);
        if let Some(reporter) = reporter {
            reporter.report_test_slow(test_case_name, duration);
        }
    }

    #[must_use]
    pub fn into_sorted(mut self) -> Self {
        self.run_diagnostics.sort_by(diagnostic_display_ordering);
        self.test_cases.sort_by(|a, b| {
            a.module_name()
                .cmp(b.module_name())
                .then_with(|| a.name().cmp(b.name()))
        });
        self.captured_outputs
            .sort_by(|a, b| a.test_name().cmp(b.test_name()));
        self
    }

    pub fn durations(&self) -> &HashMap<QualifiedFunctionName, std::time::Duration> {
        &self.durations
    }

    pub fn failed_tests(&self) -> &[QualifiedFunctionName] {
        &self.failed_tests
    }

    pub fn flaky_tests(&self) -> &[FlakyTest] {
        &self.flaky_tests
    }

    pub fn test_cases(&self) -> &[TestExecutionResult] {
        &self.test_cases
    }

    pub fn captured_outputs(&self) -> &[CapturedTestOutput] {
        &self.captured_outputs
    }
}
