//! Package-tree orchestration and run-wide execution state.

use std::cell::Cell;
use std::collections::HashMap;

use karva_coverage::CoverageSession;
use karva_python_semantic::QualifiedTestName;
use pyo3::prelude::*;

use crate::Context;
use crate::diagnostic::{fixture_resolution_diagnostic, invalid_parametrize_diagnostic};
use crate::discovery::{DiscoveredModule, DiscoveredPackage, DiscoveredTestFunction};
use crate::extensions::fixtures::FixtureScope;
use crate::runner::fixture_resolver::{FixturePlanCompiler, FixtureResolutionError};
use crate::runner::test_iterator::{CompiledTestPlan, TestVariantIterator};
use crate::runner::{FinalizerCache, FixtureCache};

mod failure;
mod fixture;
mod outcome;
mod variant;

pub use fixture::{FixtureCallError, FixtureChainEntry};

use failure::TestError;

type CompiledTestPlans = HashMap<String, Result<CompiledTestPlan, FixtureResolutionError>>;

/// Executes one discovered package tree inside an attached Python interpreter.
///
/// This type owns only run-wide state: fixture caches, coverage context, and
/// failure-budget accounting. Package traversal stays in this module; fixture
/// lifecycle and individual test-variant lifecycle live in child modules.
pub struct PackageRunner<'context, 'settings> {
    /// Shared settings, reporter, and result accumulator for this test run.
    context: &'context Context<'settings>,
    /// Fixture values retained until their declared scope completes.
    fixture_cache: FixtureCache,
    /// Fixture finalizers retained until their declared scope completes.
    finalizer_cache: FinalizerCache,
    /// Active coverage session for this worker, when coverage is enabled.
    coverage: Option<&'context CoverageSession>,
    /// Failed variants observed so far, used to enforce `max-fail`.
    failed_count: Cell<u32>,
}

impl<'context, 'settings> PackageRunner<'context, 'settings> {
    /// Creates an empty runner for one discovered package tree.
    pub(crate) fn new(
        context: &'context Context<'settings>,
        coverage: Option<&'context CoverageSession>,
    ) -> Self {
        Self {
            context,
            fixture_cache: FixtureCache::default(),
            finalizer_cache: FinalizerCache::default(),
            coverage,
            failed_count: Cell::new(0),
        }
    }

    /// Returns whether failure count reached configured scheduling budget.
    fn max_fail_reached(&self) -> bool {
        self.context
            .settings()
            .test()
            .max_fail
            .is_exceeded_by(self.failed_count.get())
    }

    /// Adds one failed variant to `max-fail` accounting.
    fn record_outcome(&self, passed: bool) {
        if !passed {
            self.failed_count
                .set(self.failed_count.get().saturating_add(1));
        }
    }

    /// Registers a discovery or setup error against one test.
    fn register_error_test(&self, test: &DiscoveredTestFunction, error: TestError) {
        self.context.register_test_case_result(
            &QualifiedTestName::new(test.name().clone(), None),
            error.into_outcome(),
            std::time::Duration::ZERO,
            None,
        );
        self.record_outcome(false);
    }

    /// Registers one shared module error against tests not blocked by `max-fail`.
    fn register_error_module_tests(&self, module: &DiscoveredModule, error: &TestError) {
        for test in module.test_functions() {
            self.register_error_test(test, error.clone());
            if self.max_fail_reached() {
                return;
            }
        }
    }

    /// Registers one shared package error throughout its remaining test tree.
    fn register_error_package_tests(&self, package: &DiscoveredPackage, error: &TestError) {
        for module in package.modules().values() {
            self.register_error_module_tests(module, error);
            if self.max_fail_reached() {
                return;
            }
        }
        for child_package in package.packages().values() {
            self.register_error_package_tests(child_package, error);
            if self.max_fail_reached() {
                return;
            }
        }
    }

    /// Validates every parametrized test before starting session fixtures.
    ///
    /// Validation is deliberately a separate tree pass: once session setup
    /// begins, invalid parametrization must not leave partially run fixtures.
    fn validate_parametrization(&self, package: &DiscoveredPackage) -> bool {
        let mut valid = true;

        for module in package.modules().values() {
            for test in module.test_functions() {
                if let Err(error) = test.tags.validate_parametrize(test.statement()) {
                    let diagnostic = invalid_parametrize_diagnostic(
                        test.source_file().clone(),
                        test.statement(),
                        &error,
                    );
                    self.register_error_test(test, TestError::new(diagnostic));
                    valid = false;
                    if self.max_fail_reached() {
                        return false;
                    }
                }
            }
        }

        for child_package in package.packages().values() {
            valid &= self.validate_parametrization(child_package);
            if self.max_fail_reached() {
                return false;
            }
        }

        valid
    }

    /// Compiles every test fixture graph before any fixture code executes.
    fn compile_test_plans(
        py: Python<'_>,
        package: &DiscoveredPackage,
        parents: &[&DiscoveredPackage],
        plans: &mut CompiledTestPlans,
    ) {
        let mut child_parents = parents.to_vec();
        child_parents.push(package);

        for module in package.modules().values() {
            for test in module.test_functions() {
                let compiler = FixturePlanCompiler::new(&child_parents, module);
                let plan = CompiledTestPlan::compile(py, test, compiler);
                plans.insert(test.name().to_string(), plan);
            }
        }

        for child_package in package.packages().values() {
            Self::compile_test_plans(py, child_package, &child_parents, plans);
        }
    }

    /// Executes all discovered tests and session-scoped fixture teardown.
    pub(crate) fn execute(&self, py: Python<'_>, session: &DiscoveredPackage) {
        if !self.validate_parametrization(session) {
            return;
        }

        let mut test_plans = HashMap::new();
        Self::compile_test_plans(py, session, &[], &mut test_plans);

        if let Err(error) = self.run_auto_use_fixtures(py, &[], session, FixtureScope::Session) {
            self.register_error_package_tests(session, &error);
            return;
        }

        self.execute_package(py, session, &[], &mut test_plans);
        self.report_scope_cleanup(py, FixtureScope::Session);
    }

    /// Executes module auto-use fixtures, variants, and module teardown.
    fn execute_module(
        &self,
        py: Python<'_>,
        module: &DiscoveredModule,
        parents: &[&DiscoveredPackage],
        test_plans: &mut CompiledTestPlans,
    ) -> bool {
        if let Err(error) = self.run_auto_use_fixtures(py, parents, module, FixtureScope::Module) {
            self.register_error_module_tests(module, &error);
            return false;
        }

        let mut passed = true;
        for test in module.test_functions() {
            let Some(test_plan) = test_plans.remove(&test.name().to_string()) else {
                passed = false;
                continue;
            };
            let test_plan = match test_plan {
                Ok(plan) => plan,
                Err(error) => {
                    self.register_error_test(
                        test,
                        TestError::new(fixture_resolution_diagnostic(error)),
                    );
                    passed = false;
                    if self.max_fail_reached() {
                        break;
                    }
                    continue;
                }
            };
            let variants = TestVariantIterator::new(test, test_plan);

            for variant in variants {
                let variant_passed = self.execute_test_variant(py, variant);
                self.record_outcome(variant_passed);
                passed &= variant_passed;

                if self.max_fail_reached() {
                    break;
                }
            }

            if self.max_fail_reached() {
                break;
            }
        }

        self.report_scope_cleanup(py, FixtureScope::Module);
        passed
    }

    /// Recursively executes package modules, child packages, and teardown.
    fn execute_package(
        &self,
        py: Python<'_>,
        package: &DiscoveredPackage,
        parents: &[&DiscoveredPackage],
        test_plans: &mut CompiledTestPlans,
    ) -> bool {
        let mut child_parents = parents.to_vec();
        child_parents.push(package);

        if package.configuration_module_impl().is_some()
            && let Err(error) =
                self.run_auto_use_fixtures(py, parents, package, FixtureScope::Package)
        {
            self.register_error_package_tests(package, &error);
            return false;
        }

        let mut passed = true;
        for module in package.modules().values() {
            passed &= self.execute_module(py, module, &child_parents, test_plans);
            if self.max_fail_reached() {
                break;
            }
        }

        if !self.max_fail_reached() {
            for child_package in package.packages().values() {
                passed &= self.execute_package(py, child_package, &child_parents, test_plans);
                if self.max_fail_reached() {
                    break;
                }
            }
        }

        self.report_scope_cleanup(py, FixtureScope::Package);
        passed
    }
}
