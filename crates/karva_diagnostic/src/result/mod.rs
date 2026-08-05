//! Serializable execution results exchanged between workers and controller.

mod case;
mod diagnostic;
mod flaky;
pub mod kind;
mod output;
mod stats;

use std::collections::{BTreeSet, HashMap};

use karva_python_semantic::TestCacheKey;
use serde::{Deserialize, Serialize};

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

/// Controller-side results containing transport-safe diagnostics.
pub type AggregatedResults = RunResults<RenderedDiagnostic>;

impl<D> RunResults<D> {
    /// Preallocates storage for the controller's measured scheduling units.
    pub fn with_test_capacity(test_capacity: usize) -> Self {
        Self {
            durations: HashMap::with_capacity(test_capacity),
            test_cases: Vec::with_capacity(test_capacity),
            ..Self::default()
        }
    }

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

        let duration = test_case.duration();
        self.durations
            .entry(cache_key)
            .and_modify(|existing_duration| *existing_duration += duration)
            .or_insert(duration);
        self.test_cases.push(test_case);
    }

    fn register_slow(&mut self) {
        self.stats.add(TestResultKind::Slow);
    }

    fn sort_results(&mut self) {
        self.test_cases.sort_by(|a, b| {
            a.module_name()
                .cmp(b.module_name())
                .then_with(|| a.name().cmp(b.name()))
        });
        self.flaky_tests.sort_by(FlakyTest::display_ordering);
    }
}

impl RunResults<RenderedDiagnostic> {
    /// Sorts cases into deterministic display order after event aggregation.
    #[must_use]
    pub fn into_sorted(mut self) -> Self {
        self.sort_results();
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
