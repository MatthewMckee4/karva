mod flaky;
mod kind;
mod stats;

use std::collections::HashMap;

use karva_python_semantic::{QualifiedFunctionName, QualifiedTestName};
use ruff_db::diagnostic::Diagnostic;

use crate::reporter::Reporter;

pub use flaky::{DisplayFlakyTest, DisplayFlakyTests, FlakyTest};
pub use kind::{IndividualTestResultKind, TestResultKind};
pub use stats::TestResultStats;

/// Represents the result of a test run.
///
/// This is held in the test context and updated throughout the test run.
#[derive(Debug, Clone, Default)]
pub struct TestRunResult {
    /// Diagnostics generated during test discovery, collection, and execution,
    /// in the order they were emitted.
    diagnostics: Vec<Diagnostic>,

    /// Stats generated during test execution.
    stats: TestResultStats,

    /// The duration of each test function.
    durations: HashMap<QualifiedFunctionName, std::time::Duration>,

    /// Names of tests that failed during this run.
    failed_tests: Vec<QualifiedFunctionName>,

    /// Tests that passed only after at least one retry.
    flaky_tests: Vec<FlakyTest>,
}

impl TestRunResult {
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn add_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn stats(&self) -> &TestResultStats {
        &self.stats
    }

    pub fn register_test_case_result(
        &mut self,
        test_case_name: &QualifiedTestName,
        result: IndividualTestResultKind,
        duration: std::time::Duration,
        reporter: Option<&dyn Reporter>,
    ) {
        self.stats.add(result.clone().into());

        let function_name = test_case_name.function_name().clone();

        if matches!(result, IndividualTestResultKind::Failed) {
            self.failed_tests.push(function_name.clone());
        }

        if let Some(reporter) = reporter {
            reporter.report_test_case_result(test_case_name, result, duration);
            reporter.notify_test_completed(test_case_name);
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
        result: &IndividualTestResultKind,
        duration: std::time::Duration,
        passed_on: u32,
        total_attempts: u32,
        reporter: Option<&dyn Reporter>,
    ) {
        self.stats.add(result.clone().into());

        let function_name = test_case_name.function_name().clone();

        if matches!(result, IndividualTestResultKind::Failed) {
            self.failed_tests.push(function_name.clone());
        } else if matches!(result, IndividualTestResultKind::Passed) {
            self.stats.add(TestResultKind::Flaky);
            self.flaky_tests.push(FlakyTest::from_qualified_name(
                test_case_name,
                passed_on,
                total_attempts,
                duration,
            ));
        }

        if let Some(reporter) = reporter {
            reporter.notify_test_completed(test_case_name);
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
        self.diagnostics.sort_by(Diagnostic::ruff_start_ordering);
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
}
