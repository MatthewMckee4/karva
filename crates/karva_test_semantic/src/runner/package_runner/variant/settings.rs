//! Derived identity and execution policy for one test variant.

use std::time::Duration;

use karva_metadata::filter::EvalContext;
use karva_metadata::{FlakyResult, JunitFlakyFailStatus};
use karva_python_semantic::QualifiedTestName;
use pyo3::prelude::*;

use crate::extensions::fixtures::NormalizedFixture;
use crate::extensions::functions::snapshot::SnapshotContext;
use crate::extensions::tags::expect_fail::ExpectFailTag;
use crate::extensions::tags::fail_slow::FailSlowTag;
use crate::extensions::tags::timeout::TimeoutTag;
use crate::runner::fixture_arguments::FixtureArguments;
use crate::utils::{test_parameters, truncate_string};

use super::VariantRunner;

/// Derived identity and execution policy shared by every retry attempt.
pub(super) struct VariantSettings {
    /// Names used by reporting, coverage, and snapshots.
    pub(super) identity: VariantIdentitySettings,
    /// Python invocation behavior for this variant.
    pub(super) execution: VariantExecutionSettings,
    /// Retry, flake, and duration policy for this variant.
    pub(super) retry: VariantRetrySettings,
}

/// Names derived once and reused by all lifecycle phases.
pub(super) struct VariantIdentitySettings {
    /// User-visible test name including parameter and fixture identity.
    pub(super) qualified_test_name: QualifiedTestName,
    /// Cached string form used by coverage.
    pub(super) qualified_name: String,
    /// Snapshot identity restored before each attempt.
    pub(super) snapshot_context: SnapshotContext,
}

/// Python invocation behavior derived from test metadata and settings.
pub(super) struct VariantExecutionSettings {
    /// Expected-failure tag needed for result classification.
    pub(super) expect_fail_tag: Option<ExpectFailTag>,
    /// Result of patching pytest-style async wrappers.
    pub(super) async_patch_result: PyResult<bool>,
    /// Whether Karva must await the returned coroutine.
    pub(super) is_async: bool,
    /// Per-call timeout in seconds, when configured.
    pub(super) timeout_seconds: Option<f64>,
}

/// Retry and duration policy derived from tags and project settings.
pub(super) struct VariantRetrySettings {
    /// Full-lifecycle failure budget, when configured.
    pub(super) fail_slow_budget: Option<Duration>,
    /// Total variant duration threshold for slow reporting.
    pub(super) slow_timeout: Option<Duration>,
    /// Total attempts, including the initial call.
    pub(super) max_attempts: u32,
    /// Whether a pass after retry fails the run.
    pub(super) flaky_result: FlakyResult,
    /// How a flaky failure appears in `JUnit`.
    pub(super) junit_flaky_fail_status: JunitFlakyFailStatus,
}

impl VariantRunner<'_, '_, '_, '_, '_> {
    /// Derives final identity and execution policy after fixture setup.
    pub(super) fn settings(
        &self,
        function_arguments: &FixtureArguments,
        known_test_name: Option<QualifiedTestName>,
    ) -> VariantSettings {
        let name = self.input.test.name();
        let fixture_names = self
            .input
            .fixtures
            .dependencies
            .iter()
            .map(|fixture_id| {
                self.input
                    .fixtures
                    .plan
                    .fixture(*fixture_id)
                    .function_name()
            })
            .collect::<Vec<_>>();
        let framework_fixture_names = self
            .input
            .fixtures
            .dependencies
            .iter()
            .map(|fixture_id| self.input.fixtures.plan.fixture(*fixture_id))
            .filter(|fixture| fixture.name().module_path().module_name() == "karva._builtins")
            .map(NormalizedFixture::function_name)
            .collect::<Vec<_>>();
        let qualified_test_name = known_test_name.unwrap_or_else(|| {
            let parameters = if let Some(id) = &self.input.identity.id {
                Some(id.clone())
            } else {
                test_parameters(
                    self.py,
                    function_arguments,
                    self.input.test.parameters(),
                    &framework_fixture_names,
                )
            };
            if let Some(parameters) = parameters {
                QualifiedTestName::with_parameters(name.clone(), parameters)
            } else {
                QualifiedTestName::new(name.clone())
            }
            .with_case_index(self.input.identity.case_index)
        });
        let qualified_name = qualified_test_name.to_string();
        let tag_names = self.input.tags.tag_names();
        let evaluation_context = EvalContext {
            test_name: &qualified_name,
            tags: &tag_names,
        };
        let fail_slow_budget = self
            .input
            .tags
            .fail_slow_tag()
            .map(FailSlowTag::seconds)
            .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
            .or_else(|| {
                self.package_runner
                    .context
                    .settings()
                    .fail_slow_for(&evaluation_context)
            });
        let slow_timeout = self
            .package_runner
            .context
            .settings()
            .slow_timeout_for(&evaluation_context);
        let timeout_seconds = self
            .input
            .tags
            .timeout_tag()
            .map(TimeoutTag::seconds)
            .or_else(|| {
                self.package_runner
                    .context
                    .settings()
                    .timeout_for(&evaluation_context)
                    .map(|duration| duration.as_secs_f64())
            });
        let max_attempts = self
            .package_runner
            .context
            .settings()
            .retry_for(&evaluation_context)
            .saturating_add(1);
        let flaky_result = self
            .package_runner
            .context
            .settings()
            .flaky_result_for(&evaluation_context);
        let junit_flaky_fail_status = self
            .package_runner
            .context
            .settings()
            .junit_flaky_fail_status_for(&evaluation_context);
        let expect_fail_tag = self.input.tags.expect_fail_tag();
        let async_patch_result = if self.input.test.is_async() {
            crate::utils::patch_async_test_function(self.py, &self.input.test.py_function)
        } else {
            Ok(false)
        };
        let is_async = self.input.test.is_async() && matches!(&async_patch_result, Ok(false));
        let mut snapshot_test_name = name.function_name().to_string();
        if self.input.identity.id.is_none() && fixture_names.is_empty() {
            if let Some(parameters) = qualified_test_name.parameters() {
                snapshot_test_name.push('(');
                snapshot_test_name.push_str(parameters);
                snapshot_test_name.push(')');
            }
        } else {
            let parameters = if !fixture_names.is_empty()
                && function_arguments.len() == fixture_names.len()
                && fixture_names
                    .iter()
                    .all(|name| function_arguments.contains(name))
            {
                let mut rendered = String::new();
                for (index, name) in fixture_names.iter().enumerate() {
                    if index > 0 {
                        rendered.push_str(", ");
                    }
                    rendered.push_str(&truncate_string(name));
                }
                Some(rendered)
            } else {
                test_parameters(
                    self.py,
                    function_arguments,
                    self.input.test.parameters(),
                    &fixture_names,
                )
            };
            if let Some(parameters) = parameters {
                snapshot_test_name.push('(');
                snapshot_test_name.push_str(&parameters);
                snapshot_test_name.push(')');
            }
        }
        let snapshot_context =
            SnapshotContext::new(self.input.module_path.to_string(), snapshot_test_name);

        VariantSettings {
            identity: VariantIdentitySettings {
                qualified_test_name,
                qualified_name,
                snapshot_context,
            },
            execution: VariantExecutionSettings {
                expect_fail_tag,
                async_patch_result,
                is_async,
                timeout_seconds,
            },
            retry: VariantRetrySettings {
                fail_slow_budget,
                slow_timeout,
                max_attempts,
                flaky_result,
                junit_flaky_fail_status,
            },
        }
    }
}
