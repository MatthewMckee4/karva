//! Serializable execution results exchanged between workers and controller.

mod case;
mod diagnostic;
mod flaky;
pub mod kind;
mod output;
mod stats;

use std::collections::{BTreeSet, HashMap};

use camino::Utf8Path;
use karva_python_semantic::{QualifiedTestName, TestCacheKey};
use serde::{Deserialize, Serialize};

use crate::reporter::Reporter;
use crate::{Diagnostic, DisplayDiagnosticConfig, render_diagnostic};
use kind::TestResultKind;

pub use case::{
    FixtureFailure, FixtureUsage, TestCaseAttempt, TestCaseOutcome, TestCaseResult, TestCaseRetry,
    TestExecutionAttempt, TestExecutionOutcome, TestExecutionResult,
};
pub use diagnostic::RenderedDiagnostic;
pub use flaky::{DisplayFlakyTests, FlakyTest};
pub use kind::IndividualTestResultKind;
pub use output::CapturedTestOutput;
pub use stats::TestResultStats;

/// Results from one or more workers, parameterized by diagnostic representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResults<D> {
    /// Diagnostics that describe the run rather than one test case.
    pub run_diagnostics: Vec<D>,

    /// Stats generated during test execution.
    pub stats: TestResultStats,

    /// Duration of each schedulable test or parameter case.
    pub durations: HashMap<TestCacheKey, std::time::Duration>,

    /// Scheduling keys for failed test variants.
    pub failed_tests: BTreeSet<TestCacheKey>,

    /// Tests that passed only after one or more failed attempts.
    pub flaky_tests: Vec<FlakyTest>,

    /// Final outcome for each executed test variant.
    pub test_cases: Vec<TestCaseResult<D>>,
}

impl<D> Default for RunResults<D> {
    fn default() -> Self {
        Self {
            run_diagnostics: Vec::new(),
            stats: TestResultStats::default(),
            durations: HashMap::new(),
            failed_tests: BTreeSet::new(),
            flaky_tests: Vec::new(),
            test_cases: Vec::new(),
        }
    }
}

/// Worker-side results retaining source-backed diagnostics.
pub type TestRunResult = RunResults<Diagnostic>;

/// Controller-side results containing transport-safe diagnostics.
pub type AggregatedResults = RunResults<RenderedDiagnostic>;

/// Orders diagnostics for display.
///
/// Diagnostics with a source file sort by source and span; span-less diagnostics
/// sort after them by code and message.
fn diagnostic_display_ordering(a: &Diagnostic, b: &Diagnostic) -> std::cmp::Ordering {
    match (a.primary_annotation(), b.primary_annotation()) {
        (Some(a), Some(b)) => a
            .span()
            .source_file()
            .cmp(b.span().source_file())
            .then_with(|| a.span().range().start().cmp(&b.span().range().start()))
            .then_with(|| a.span().range().end().cmp(&b.span().range().end())),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a
            .code()
            .cmp(b.code())
            .then_with(|| a.primary_message().cmp(b.primary_message())),
    }
}

impl<D> RunResults<D> {
    pub fn stats(&self) -> &TestResultStats {
        &self.stats
    }

    fn register_case(&mut self, cache_key: TestCacheKey, test_case: TestCaseResult<D>) {
        let result = test_case.outcome().result_kind();
        self.stats.add(result.clone().into());

        if matches!(
            result,
            IndividualTestResultKind::Failed | IndividualTestResultKind::Error
        ) {
            self.failed_tests.insert(cache_key.clone());
        } else if let Some(retry) = test_case.retry()
            && matches!(result, IndividualTestResultKind::Passed)
        {
            self.stats.add(TestResultKind::Flaky);
            self.flaky_tests.push(FlakyTest::from_display_name(
                test_case.module_name(),
                test_case.name(),
                retry.attempts(),
                retry.max_attempts(),
                test_case.duration(),
            ));
        }

        self.durations
            .entry(cache_key)
            .and_modify(|existing_duration| *existing_duration += test_case.duration())
            .or_insert(test_case.duration());
        self.test_cases.push(test_case);
    }

    pub(crate) fn register_slow(&mut self) {
        self.stats.add(TestResultKind::Slow);
    }

    fn sort_test_cases(&mut self) {
        self.test_cases.sort_by(|a, b| {
            a.module_name()
                .cmp(b.module_name())
                .then_with(|| a.name().cmp(b.name()))
        });
    }
}

impl RunResults<Diagnostic> {
    /// Adds a diagnostic not owned by one test case.
    pub fn add_run_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.run_diagnostics.push(diagnostic);
    }

    /// Records one final test outcome and reports it immediately when requested.
    pub fn register_test_case_result(
        &mut self,
        test_case_name: &QualifiedTestName,
        outcome: TestExecutionOutcome,
        duration: std::time::Duration,
        captured_output: Option<CapturedTestOutput>,
        reporter: Option<&dyn Reporter>,
    ) {
        let cache_key = test_case_name.cache_key();
        let test_case = TestCaseResult::new(test_case_name, outcome, duration, captured_output);

        if let Some(reporter) = reporter {
            reporter.report_test_case_result(
                test_case_name,
                test_case.outcome().result_kind(),
                duration,
            );
            reporter.report_test_completed(&cache_key, &test_case);
        }
        self.register_case(cache_key, test_case);
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
        reporter: Option<&dyn Reporter>,
    ) {
        let cache_key = test_case_name.cache_key();
        let test_case = TestCaseResult::retried(
            test_case_name,
            outcome,
            duration,
            retry,
            captured_output,
            attempts,
        );
        if let Some(reporter) = reporter {
            reporter.report_test_completed(&cache_key, &test_case);
        }
        self.register_case(cache_key, test_case);
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
        self.register_slow();
        if let Some(reporter) = reporter {
            reporter.report_test_slow(test_case_name, duration);
        }
    }

    /// Sorts diagnostics and cases into deterministic display order.
    #[must_use]
    pub fn into_sorted(mut self) -> Self {
        self.run_diagnostics.sort_by(diagnostic_display_ordering);
        self.sort_test_cases();
        self
    }

    /// Converts source-backed diagnostics into transport-safe renderings.
    pub fn render(self, cwd: &Utf8Path, config: DisplayDiagnosticConfig) -> AggregatedResults {
        AggregatedResults {
            run_diagnostics: self
                .run_diagnostics
                .iter()
                .map(|diagnostic| render_diagnostic(diagnostic, cwd, config))
                .collect(),
            stats: self.stats,
            durations: self.durations,
            failed_tests: self.failed_tests,
            flaky_tests: self.flaky_tests,
            test_cases: self
                .test_cases
                .into_iter()
                .map(|case| case.render(cwd, config))
                .collect(),
        }
    }
}

impl RunResults<RenderedDiagnostic> {
    /// Sorts cases into deterministic display order after event aggregation.
    #[must_use]
    pub fn into_sorted(mut self) -> Self {
        self.sort_test_cases();
        self
    }

    /// Records one worker-rendered test outcome.
    pub fn register_rendered_test_case(
        &mut self,
        cache_key: TestCacheKey,
        test_case: TestCaseResult,
    ) {
        self.register_case(cache_key, test_case);
    }

    /// Records one worker-reported slow test.
    pub fn register_slow_test(&mut self) {
        self.register_slow();
    }

    /// Adds one deduplicated worker-rendered run diagnostic.
    pub fn add_rendered_run_diagnostic(&mut self, diagnostic: RenderedDiagnostic) {
        if !self.run_diagnostics.contains(&diagnostic) {
            self.run_diagnostics.push(diagnostic);
        }
    }

    /// Whether every test and run-level diagnostic completed successfully.
    pub fn is_success(&self) -> bool {
        self.stats.is_success() && !self.has_run_errors()
    }

    /// Whether collection or worker infrastructure produced an error diagnostic.
    pub fn has_run_errors(&self) -> bool {
        self.run_diagnostics
            .iter()
            .any(RenderedDiagnostic::is_error)
    }

    /// Whether any test exhausted retries without passing.
    pub fn has_flaky_failures(&self) -> bool {
        self.test_cases.iter().any(TestCaseResult::is_flaky_failure)
    }

    /// Records a test interrupted by controller shutdown as a failed result.
    pub fn register_interrupted_test(&mut self, name: &str, duration: std::time::Duration) {
        let cache_key = interrupted_test_cache_key(name);
        self.register_case(
            cache_key,
            TestCaseResult::from_display_name(
                name,
                TestCaseOutcome::failed(RenderedDiagnostic::interrupted(name)),
                duration,
                None,
            ),
        );
    }
}

fn interrupted_test_cache_key(name: &str) -> TestCacheKey {
    let Some((module, function)) = name.rsplit_once("::") else {
        return TestCacheKey::function_name(name);
    };
    let function = function
        .split_once('(')
        .or_else(|| function.split_once('['))
        .map_or(function, |(base, _)| base);
    TestCacheKey::function_name(&format!("{module}::{function}"))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::Severity;

    #[test]
    fn registers_worker_result() {
        let cache_key = TestCacheKey::function_name("mod::test_failure");
        let result = TestCaseResult::from_display_name(
            "mod::test_failure(value=1)",
            TestCaseOutcome::failed(RenderedDiagnostic::new(
                "test-failure",
                Severity::Error,
                "failed",
                "error[test-failure]: failed\n",
            )),
            Duration::from_millis(10),
            None,
        );
        let mut aggregated = AggregatedResults::default();

        aggregated.register_rendered_test_case(cache_key.clone(), result);

        assert_eq!(aggregated.stats.failed(), 1);
        assert_eq!(aggregated.failed_tests, BTreeSet::from([cache_key.clone()]));
        assert_eq!(aggregated.durations[&cache_key], Duration::from_millis(10));
    }

    #[test]
    fn interrupted_parameters_use_base_name_for_history() {
        let mut aggregated = AggregatedResults::default();

        aggregated.register_interrupted_test("mod::test_slow(value=1)", Duration::from_millis(24));

        let cache_key = TestCacheKey::function_name("mod::test_slow");
        assert_eq!(aggregated.failed_tests, BTreeSet::from([cache_key.clone()]));
        assert!(aggregated.durations.contains_key(&cache_key));
    }
}
