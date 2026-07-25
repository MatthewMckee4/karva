//! Package-tree orchestration and run-wide execution state.

use std::cell::Cell;

use karva_coverage::CoverageSession;
use karva_diagnostic::TestExecutionOutcome;
use karva_python_semantic::QualifiedTestName;
use pyo3::prelude::*;
use ruff_db::diagnostic::Diagnostic;

use crate::Context;
use crate::diagnostic::{fixture_resolution_diagnostic, invalid_parametrize_diagnostic};
use crate::discovery::{DiscoveredModule, DiscoveredPackage, DiscoveredTestFunction};
use crate::extensions::fixtures::FixtureScope;
use crate::runner::fixture_resolver::RuntimeFixtureResolver;
use crate::runner::test_iterator::TestVariantIterator;
use crate::runner::{FinalizerCache, FixtureCache};
use crate::unhandled_exceptions::UnhandledExceptionCapture;

mod fixture;
mod outcome;
mod variant;

pub use fixture::{FixtureCallError, FixtureChainEntry};

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
    /// Captures exceptions that Python cannot propagate through the test call.
    exception_capture: Option<&'context UnhandledExceptionCapture>,
    /// Failed variants observed so far, used to enforce `max-fail`.
    failed_count: Cell<u32>,
}

impl<'context, 'settings> PackageRunner<'context, 'settings> {
    /// Creates an empty runner for one discovered package tree.
    pub(crate) fn new(
        context: &'context Context<'settings>,
        coverage: Option<&'context CoverageSession>,
        exception_capture: Option<&'context UnhandledExceptionCapture>,
    ) -> Self {
        Self {
            context,
            fixture_cache: FixtureCache::default(),
            finalizer_cache: FinalizerCache::default(),
            coverage,
            exception_capture,
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
    fn register_error_test(
        &self,
        test: &DiscoveredTestFunction,
        diagnostic: Diagnostic,
        related: Vec<Diagnostic>,
    ) {
        self.context.register_test_case_result(
            &QualifiedTestName::new(test.name.clone(), None),
            TestExecutionOutcome::error_with_related(diagnostic, related),
            std::time::Duration::ZERO,
            None,
        );
        self.record_outcome(false);
    }

    /// Registers one shared module error against tests not blocked by `max-fail`.
    fn register_error_module_tests(
        &self,
        module: &DiscoveredModule,
        diagnostic: &Diagnostic,
        related: &[Diagnostic],
    ) {
        for test in module.test_functions() {
            self.register_error_test(test, diagnostic.clone(), related.to_vec());
            if self.max_fail_reached() {
                return;
            }
        }
    }

    /// Registers one shared package error throughout its remaining test tree.
    fn register_error_package_tests(
        &self,
        package: &DiscoveredPackage,
        diagnostic: &Diagnostic,
        related: &[Diagnostic],
    ) {
        for module in package.modules().values() {
            self.register_error_module_tests(module, diagnostic, related);
            if self.max_fail_reached() {
                return;
            }
        }
        for child_package in package.packages().values() {
            self.register_error_package_tests(child_package, diagnostic, related);
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
                if let Err(error) = test.tags.validate_parametrize(&test.stmt_function_def) {
                    let diagnostic = invalid_parametrize_diagnostic(
                        test.source_file.clone(),
                        &test.stmt_function_def,
                        &error,
                    );
                    self.register_error_test(test, diagnostic, Vec::new());
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

    /// Executes all discovered tests and session-scoped fixture teardown.
    pub(crate) fn execute(&self, py: Python<'_>, session: &DiscoveredPackage) {
        if !self.validate_parametrization(session) {
            return;
        }

        if let Err(mut diagnostics) =
            self.run_auto_use_fixtures(py, &[], session, FixtureScope::Session)
        {
            let diagnostic = diagnostics.remove(0);
            self.register_error_package_tests(session, &diagnostic, &diagnostics);
            return;
        }

        self.execute_package(py, session, &[]);
        self.report_scope_cleanup(py, FixtureScope::Session);
    }

    /// Executes module auto-use fixtures, variants, and module teardown.
    fn execute_module(
        &self,
        py: Python<'_>,
        module: &DiscoveredModule,
        parents: &[&DiscoveredPackage],
    ) -> bool {
        if let Err(mut diagnostics) =
            self.run_auto_use_fixtures(py, parents, module, FixtureScope::Module)
        {
            let diagnostic = diagnostics.remove(0);
            self.register_error_module_tests(module, &diagnostic, &diagnostics);
            return false;
        }

        let mut passed = true;
        for test in module.test_functions() {
            let mut resolver = RuntimeFixtureResolver::new(parents, module);
            let variants = match TestVariantIterator::new(py, test, &mut resolver) {
                Ok(variants) => variants,
                Err(error) => {
                    self.register_error_test(
                        test,
                        fixture_resolution_diagnostic(error),
                        Vec::new(),
                    );
                    passed = false;
                    if self.max_fail_reached() {
                        break;
                    }
                    continue;
                }
            };

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
    ) -> bool {
        let mut child_parents = parents.to_vec();
        child_parents.push(package);

        if let Some(configuration_module) = package.configuration_module_impl()
            && let Err(mut diagnostics) =
                self.run_auto_use_fixtures(py, parents, configuration_module, FixtureScope::Package)
        {
            let diagnostic = diagnostics.remove(0);
            self.register_error_package_tests(package, &diagnostic, &diagnostics);
            return false;
        }

        let mut passed = true;
        for module in package.modules().values() {
            passed &= self.execute_module(py, module, &child_parents);
            if self.max_fail_reached() {
                break;
            }
        }

        if !self.max_fail_reached() {
            for child_package in package.packages().values() {
                passed &= self.execute_package(py, child_package, &child_parents);
                if self.max_fail_reached() {
                    break;
                }
            }
        }

        self.report_scope_cleanup(py, FixtureScope::Package);
        passed
    }
}
