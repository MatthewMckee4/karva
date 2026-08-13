//! Orchestration and reporting for one concrete test variant.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use karva_coverage::CoveragePhase;
use karva_diagnostic::TestExecutionOutcome;
use karva_metadata::RunIgnoredMode;
use karva_metadata::filter::EvalContext;
use karva_python_semantic::QualifiedTestName;
use pyo3::prelude::*;

use crate::output_capture::PythonOutputCapture;
use crate::runner::test_iterator::TestVariant;
use crate::utils::{set_attempt_env, set_test_name_env};

use super::PackageRunner;

mod attempt;
mod identity;
mod input;
mod reporting;
mod settings;

use attempt::{PreparedTestAttempt, TestLifecycleAttempt};
use input::VariantInput;
use settings::VariantSettings;

impl PackageRunner<'_, '_> {
    /// Runs one concrete parameter and fixture combination.
    pub(super) fn execute_test_variant(
        &mut self,
        py: Python<'_>,
        variant: TestVariant<'_>,
    ) -> bool {
        VariantRunner::new(self, py, variant).execute()
    }
}

/// Owns all state that remains stable while one test variant retries.
///
/// Keeping variant state separate from [`PackageRunner`] makes retry behavior
/// local: run-wide caches stay on the package runner, while parameter values,
/// fixture selections, tags, and identity stay here.
struct VariantRunner<'runner, 'context, 'settings, 'test, 'py> {
    /// Run-wide owner for fixtures, reporting, settings, and coverage.
    package_runner: &'runner mut PackageRunner<'context, 'settings>,
    /// Attached Python interpreter used by every attempt.
    py: Python<'py>,
    /// Variant inputs shared by setup, retries, and reporting.
    input: VariantInput<'test>,
}

impl<'runner, 'context, 'settings, 'test, 'py>
    VariantRunner<'runner, 'context, 'settings, 'test, 'py>
{
    /// Captures raw variant inputs before any fixture setup begins.
    fn new(
        package_runner: &'runner mut PackageRunner<'context, 'settings>,
        py: Python<'py>,
        variant: TestVariant<'test>,
    ) -> Self {
        Self {
            package_runner,
            py,
            input: VariantInput::from_test_variant(variant),
        }
    }

    /// Executes setup, all required attempts, teardown, and final reporting.
    fn execute(mut self) -> bool {
        let unresolved_test_name = self.unresolved_test_name();
        if self
            .package_runner
            .context
            .should_resume_skip(&unresolved_test_name.cache_key())
        {
            return true;
        }
        if let Some(result) = self.should_skip(&unresolved_test_name) {
            return result;
        }

        let (initial_test_name, initial_name_is_exact) =
            self.initial_test_name(unresolved_test_name);
        // Flush checkpoint before fixture setup; it covers setup and test body.
        self.package_runner
            .context
            .report_test_started(&initial_test_name);
        let retry_params = self.input.params.clone();
        let first_params = std::mem::take(&mut self.input.params);
        self.begin_pending_coverage_setup();
        let first_attempt = self.prepare_attempt(first_params, self.start_output_capture());
        let settings = if initial_name_is_exact {
            self.settings(
                &first_attempt.fixtures.function_arguments,
                Some(initial_test_name),
            )
        } else {
            let settings = self.settings(&first_attempt.fixtures.function_arguments, None);
            if settings.identity.qualified_test_name != initial_test_name {
                self.package_runner
                    .context
                    .report_test_identified(&settings.identity.qualified_test_name);
            }
            settings
        };
        self.resolve_pending_coverage_setup(&settings.identity.qualified_name);
        let function = self.input.test.py_function.clone_ref(self.py);
        let test_name_env_result =
            set_test_name_env(self.py, &settings.identity.qualified_test_name.to_string());

        tracing::debug!("Running test `{}`", settings.identity.qualified_test_name);
        let mut attempt_number = 1;
        let mut prepared_attempt = Some(first_attempt);
        let mut prior_attempts = Vec::new();

        let final_attempt = loop {
            let attempt_env_result =
                set_attempt_env(self.py, attempt_number, settings.retry.max_attempts);
            let prepared = prepared_attempt
                .take()
                .unwrap_or_else(|| self.prepare_retry(retry_params.clone(), &settings));
            self.set_coverage_context(&settings.identity.qualified_name, CoveragePhase::Run);
            let attempt = self.execute_attempt(
                &settings,
                &function,
                &test_name_env_result,
                attempt_env_result,
                prepared,
                attempt_number,
            );

            if attempt.retryable && attempt_number < settings.retry.max_attempts {
                self.package_runner.context.report_test_attempt(
                    &settings.identity.qualified_test_name,
                    attempt_number,
                    attempt.lifecycle.outcome.result_kind(),
                    attempt.lifecycle.duration,
                );
                prior_attempts.push(attempt.lifecycle);
                tracing::debug!("Retrying test `{}`", settings.identity.qualified_test_name);
                attempt_number += 1;
            } else {
                break attempt.lifecycle;
            }
        };

        self.finish(&settings, prior_attempts, final_attempt)
    }

    /// Prepares fixture values and records setup duration for one attempt.
    fn prepare_attempt(
        &mut self,
        params: HashMap<String, Arc<Py<PyAny>>>,
        output_capture: Option<PythonOutputCapture>,
    ) -> PreparedTestAttempt {
        let setup_start = Instant::now();
        let fixtures = self.package_runner.prepare_test_fixtures(
            self.py,
            &self.input.fixtures.plan,
            &self.input.fixtures.dependencies,
            &self.input.fixtures.use_dependencies,
            &self.input.fixtures.auto_use,
            params,
        );
        PreparedTestAttempt {
            fixtures,
            setup_duration: setup_start.elapsed(),
            output_capture,
        }
    }

    /// Prepares a retry under its setup coverage context.
    fn prepare_retry(
        &mut self,
        params: HashMap<String, Arc<Py<PyAny>>>,
        settings: &VariantSettings,
    ) -> PreparedTestAttempt {
        self.set_coverage_context(&settings.identity.qualified_name, CoveragePhase::Setup);
        self.prepare_attempt(params, self.start_output_capture())
    }

    /// Returns a registered skip result when filters or skip policy exclude this variant.
    fn should_skip(&self, qualified: &QualifiedTestName) -> Option<bool> {
        let filter = &self.package_runner.context.settings().test().filter;
        let run_ignored = self.package_runner.context.settings().test().run_ignored;

        if !filter.is_empty() {
            let display_name = qualified.to_string();
            let tag_names = self.input.tags.tag_names();
            let context = EvalContext {
                test_name: &display_name,
                tags: &tag_names,
            };
            if !filter.matches(&context) {
                return Some(self.package_runner.context.register_test_case_result(
                    qualified,
                    TestExecutionOutcome::Skipped { reason: None },
                    Duration::ZERO,
                    None,
                ));
            }
        }

        let skipped = match run_ignored {
            RunIgnoredMode::Default => self.input.tags.should_skip(),
            RunIgnoredMode::Only => {
                let (should_skip, _) = self.input.tags.should_skip();
                (!should_skip, None)
            }
            RunIgnoredMode::All => (false, None),
        };
        let (true, reason) = skipped else {
            return None;
        };
        Some(self.package_runner.context.register_test_case_result(
            qualified,
            TestExecutionOutcome::Skipped { reason },
            Duration::ZERO,
            None,
        ))
    }

    /// Starts best-effort Python output capture when terminal output is hidden.
    fn start_output_capture(&self) -> Option<PythonOutputCapture> {
        if self
            .package_runner
            .context
            .settings()
            .terminal()
            .show_python_output
        {
            return None;
        }

        match PythonOutputCapture::start(self.py) {
            Ok(capture) => Some(capture),
            Err(error) => {
                tracing::warn!("failed to start Python output capture: {error}");
                None
            }
        }
    }

    /// Marks setup whose full fixture-derived test identity is not known yet.
    fn begin_pending_coverage_setup(&self) {
        if let Some(coverage) = self.package_runner.coverage {
            coverage.begin_pending_test_setup(self.py);
        }
    }

    /// Resolves pending setup observations to the final qualified test identity.
    fn resolve_pending_coverage_setup(&self, test: &str) {
        if let Some(coverage) = self.package_runner.coverage {
            coverage.resolve_pending_test_setup(self.py, test);
        }
    }

    /// Updates coverage attribution for one test lifecycle phase.
    fn set_coverage_context(&self, test: &str, phase: CoveragePhase) {
        if let Some(coverage) = self.package_runner.coverage {
            coverage.set_test_context(self.py, test, phase);
        }
    }

    /// Restores session attribution between test variants.
    fn clear_coverage_context(&self) {
        if let Some(coverage) = self.package_runner.coverage {
            coverage.clear_test_context(self.py);
        }
    }
}
