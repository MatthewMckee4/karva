//! Controller-side result aggregation and worker serialization boundary.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use camino::Utf8Path;
use karva_python_semantic::TestCacheKey;
use serde::{Deserialize, Serialize};

use crate::render::render_diagnostic;
use crate::result::TestRunResultParts;
use crate::result::kind::TestResultKind;
use crate::{
    DisplayDiagnosticConfig, FlakyTest, RenderedDiagnostic, TestCaseOutcome, TestCaseResult,
    TestResultStats, TestRunResult,
};

/// Test results merged across every worker in one invocation.
#[derive(Default)]
pub struct AggregatedResults {
    /// Outcome counters merged across every worker.
    pub stats: TestResultStats,

    /// Collection and infrastructure diagnostics not owned by one test case.
    pub run_diagnostics: Vec<RenderedDiagnostic>,

    /// Base test names used to seed the next run's `--last-failed` selection.
    pub failed_tests: Vec<TestCacheKey>,

    /// Tests that passed only after one or more failed attempts.
    pub flaky_tests: Vec<FlakyTest>,

    /// Full result and retry history for every executed test case.
    pub test_cases: Vec<TestCaseResult>,

    /// Total duration keyed by unparameterized qualified test name.
    pub durations: HashMap<TestCacheKey, Duration>,
}

/// Serializable result payload transferred from one worker to the controller.
#[derive(Default, Serialize, Deserialize)]
pub struct WorkerResults {
    stats: TestResultStats,
    run_diagnostics: Vec<RenderedDiagnostic>,
    #[serde(default)]
    failed_tests: Vec<TestCacheKey>,
    test_cases: Vec<TestCaseResult>,
    durations: HashMap<TestCacheKey, Duration>,
}

impl AggregatedResults {
    /// Whether every test and run-level diagnostic completed successfully.
    pub fn is_success(&self) -> bool {
        self.stats.is_success()
            && !self
                .run_diagnostics
                .iter()
                .any(RenderedDiagnostic::is_error)
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
    pub fn register_interrupted_test(&mut self, name: &str, duration: Duration) {
        let function_name = base_test_name(name);
        self.stats.add(TestResultKind::Failed);
        self.failed_tests.push(function_name.clone());
        self.test_cases.push(TestCaseResult::from_display_name(
            name,
            TestCaseOutcome::failed(RenderedDiagnostic::interrupted(name)),
            duration,
            None,
        ));
        self.durations.insert(function_name, duration);
    }

    /// Merges one completed worker payload into this run.
    pub fn merge_worker(&mut self, worker: WorkerResults) {
        for diagnostic in worker.run_diagnostics {
            if !self.run_diagnostics.contains(&diagnostic) {
                self.run_diagnostics.push(diagnostic);
            }
        }
        self.stats.merge(&worker.stats);
        self.failed_tests.extend(worker.failed_tests);
        if worker.stats.flaky() > 0 {
            for case in &worker.test_cases {
                if let Some(retry) = case.retry()
                    && matches!(case.outcome(), TestCaseOutcome::Passed)
                {
                    self.flaky_tests.push(FlakyTest::from_display_name(
                        case.module_name(),
                        case.name(),
                        retry.attempts(),
                        retry.max_attempts(),
                        case.duration(),
                    ));
                }
            }
        }
        self.test_cases.extend(worker.test_cases);
        self.durations.extend(worker.durations);
    }
}

/// Renders worker-owned diagnostics and converts semantic names for transport.
pub fn render_worker_results(
    result: TestRunResult,
    cwd: &Utf8Path,
    config: &DisplayDiagnosticConfig,
) -> Result<WorkerResults> {
    let TestRunResultParts {
        run_diagnostics,
        stats,
        durations,
        failed_tests,
        test_cases,
    } = result.into_parts();
    let run_diagnostics = run_diagnostics
        .iter()
        .map(|diagnostic| render_diagnostic(diagnostic, cwd, *config))
        .collect::<Vec<_>>();
    let test_cases = test_cases
        .into_iter()
        .map(|case| {
            case.try_map_diagnostic(|diagnostic| {
                Ok::<_, anyhow::Error>(render_diagnostic(diagnostic, cwd, *config))
            })
        })
        .collect::<Result<Vec<TestCaseResult>>>()?;

    let mut failed_tests = failed_tests.into_iter().collect::<Vec<_>>();
    failed_tests.sort();

    Ok(WorkerResults {
        stats,
        run_diagnostics,
        failed_tests,
        test_cases,
        durations,
    })
}

fn base_test_name(name: &str) -> TestCacheKey {
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
    use super::*;
    use crate::Severity;

    #[test]
    fn merges_worker_failures_and_durations() {
        let mut stats = TestResultStats::default();
        stats.add(TestResultKind::Failed);
        let worker = WorkerResults {
            stats,
            failed_tests: vec![TestCacheKey::function_name("mod::test_failure")],
            test_cases: vec![TestCaseResult::from_display_name(
                "mod::test_failure(value=1)",
                TestCaseOutcome::failed(RenderedDiagnostic::new(
                    "test-failure",
                    Severity::Error,
                    "failed",
                    "error[test-failure]: failed\n",
                )),
                Duration::from_millis(10),
                None,
            )],
            durations: HashMap::from([(
                TestCacheKey::function_name("mod::test_failure"),
                Duration::from_millis(10),
            )]),
            ..WorkerResults::default()
        };
        let mut aggregated = AggregatedResults::default();

        aggregated.merge_worker(worker);

        assert_eq!(aggregated.stats.failed(), 1);
        assert_eq!(
            aggregated.failed_tests,
            [TestCacheKey::function_name("mod::test_failure")]
        );
        assert_eq!(
            aggregated.durations["mod::test_failure"],
            Duration::from_millis(10)
        );
    }

    #[test]
    fn interrupted_parameters_use_base_name_for_history() {
        let mut aggregated = AggregatedResults::default();

        aggregated.register_interrupted_test("mod::test_slow(value=1)", Duration::from_millis(24));

        assert_eq!(
            aggregated.failed_tests,
            [TestCacheKey::function_name("mod::test_slow")]
        );
        assert!(aggregated.durations.contains_key("mod::test_slow"));
    }
}
