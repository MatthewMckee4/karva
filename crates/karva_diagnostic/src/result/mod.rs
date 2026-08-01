mod case;
mod diagnostic;
mod flaky;
mod kind;
mod output;
mod stats;

use std::collections::HashMap;

use karva_python_semantic::{QualifiedFunctionName, QualifiedTestName};
use ruff_db::diagnostic::Diagnostic;

use crate::reporter::Reporter;

pub use case::{
    FixtureFailure, FixtureUsage, TestCaseAttempt, TestCaseOutcome, TestCaseResult, TestCaseRetry,
    TestExecutionAttempt, TestExecutionOutcome, TestExecutionResult,
};
pub use diagnostic::RenderedDiagnostic;
pub use flaky::{DisplayFlakyTest, DisplayFlakyTests, FlakyTest};
pub use kind::{IndividualTestResultKind, TestResultKind};
pub use output::CapturedTestOutput;
pub use stats::TestResultStats;

/// Represents the result of a test run.
///
/// This is held in the test context and updated throughout the test run.
#[derive(Debug, Clone, Default)]
pub struct TestRunResult {
    /// Diagnostics that describe the run rather than one test case.
    run_diagnostics: Vec<Diagnostic>,

    /// Stats generated during test execution.
    stats: TestResultStats,

    /// The duration of each test function.
    durations: HashMap<QualifiedFunctionName, std::time::Duration>,

    /// Final outcome for each executed test variant.
    test_cases: Vec<TestExecutionResult>,
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

    pub fn register_test_case_result(
        &mut self,
        test_case_name: &QualifiedTestName,
        outcome: TestExecutionOutcome,
        duration: std::time::Duration,
        captured_output: Option<CapturedTestOutput>,
        reporter: Option<&dyn Reporter>,
    ) {
        let result = outcome.result_kind();
        self.stats.add(result.clone().into());

        let function_name = test_case_name.function_name().clone();
        self.test_cases.push(TestCaseResult::new(
            test_case_name,
            outcome,
            duration,
            captured_output,
        ));

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
    /// When the final outcome is `Passed`, the test is counted as flaky.
    pub fn register_retried_result(
        &mut self,
        test_case_name: &QualifiedTestName,
        outcome: TestExecutionOutcome,
        duration: std::time::Duration,
        retry: TestCaseRetry,
        captured_output: Option<CapturedTestOutput>,
        attempts: Vec<TestExecutionAttempt>,
    ) {
        let result = outcome.result_kind();
        self.stats.add(result.clone().into());

        let function_name = test_case_name.function_name().clone();
        self.test_cases.push(TestCaseResult::retried(
            test_case_name,
            outcome,
            duration,
            retry,
            captured_output,
            attempts,
        ));

        if matches!(&result, IndividualTestResultKind::Passed) {
            self.stats.add(TestResultKind::Flaky);
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
        self
    }

    pub fn durations(&self) -> &HashMap<QualifiedFunctionName, std::time::Duration> {
        &self.durations
    }

    pub fn test_cases(&self) -> &[TestExecutionResult] {
        &self.test_cases
    }

    pub fn into_parts(
        self,
    ) -> (
        Vec<Diagnostic>,
        TestResultStats,
        HashMap<QualifiedFunctionName, std::time::Duration>,
        Vec<TestExecutionResult>,
    ) {
        (
            self.run_diagnostics,
            self.stats,
            self.durations,
            self.test_cases,
        )
    }
}
