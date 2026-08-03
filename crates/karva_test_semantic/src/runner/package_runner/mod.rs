//! Package-tree orchestration and run-wide execution state.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use camino::Utf8PathBuf;
use karva_coverage::CoverageSession;
use karva_python_semantic::QualifiedTestName;
use pyo3::prelude::*;

use crate::diagnostic::{fixture_resolution_diagnostic, invalid_parametrize_diagnostic};
use crate::discovery::{DiscoveredModule, DiscoveredPackage, DiscoveredTestFunction};
use crate::extensions::fixtures::FixtureScope;
use crate::runner::fixture_resolver::{FixturePlanCompiler, FixtureResolutionError};
use crate::runner::request::RequestState;
use crate::runner::scoped_storage::ScopeKey;
use crate::runner::test_iterator::{
    CompiledTestPlan, TestVariant, TestVariantIterator, reorder_variant_ref_indices,
    reorder_variants,
};
use crate::runner::{FinalizerCache, FixtureCache};
use crate::{Context, RunState};

mod failure;
mod fixture;
mod outcome;
mod variant;

pub use fixture::{FixtureCallError, FixtureChainEntry};

use failure::TestError;

type CompiledTestPlans = HashMap<String, Result<CompiledTestPlan, FixtureResolutionError>>;

#[derive(Default)]
struct CompilationFeatures {
    uses_request: bool,
    requires_reordering: bool,
    requires_cross_module_reordering: bool,
}

impl CompilationFeatures {
    fn include(&mut self, plan: &CompiledTestPlan) {
        self.uses_request |= plan.uses_request();
        self.requires_reordering |= plan.requires_reordering();
        self.requires_cross_module_reordering |= plan.requires_cross_module_reordering();
    }
}

struct ScheduledModule<'a> {
    module: &'a DiscoveredModule,
    packages: Vec<&'a DiscoveredPackage>,
}

struct ScheduledVariant<'a> {
    module: usize,
    variant: TestVariant<'a>,
}

/// Executes one discovered package tree inside an attached Python interpreter.
///
/// This type owns only run-wide state: fixture caches, coverage context, and
/// failure-budget accounting. Package traversal stays in this module; fixture
/// lifecycle and individual test-variant lifecycle live in child modules.
pub struct PackageRunner<'context, 'settings> {
    /// Shared immutable settings and reporting services for this test run.
    context: &'context Context<'settings>,
    /// Result accumulator exclusively owned by this runner during execution.
    state: &'context mut RunState,
    /// Fixture values retained until their declared scope completes.
    fixture_cache: Rc<RefCell<FixtureCache>>,
    /// Fixture finalizers retained until their declared scope completes.
    finalizer_cache: Rc<RefCell<FinalizerCache>>,
    /// Shared request collection state, allocated only when one test uses it.
    request_state: Option<Box<Result<RefCell<RequestState>, String>>>,
    /// Active coverage session for this worker, when coverage is enabled.
    coverage: Option<&'context CoverageSession>,
    /// Failed variants observed so far, used to enforce `max-fail`.
    failed_count: u32,
}

impl<'context, 'settings> PackageRunner<'context, 'settings> {
    /// Creates an empty runner for one discovered package tree.
    pub(crate) fn new(
        context: &'context Context<'settings>,
        state: &'context mut RunState,
        coverage: Option<&'context CoverageSession>,
    ) -> Self {
        Self {
            context,
            state,
            fixture_cache: Rc::new(RefCell::new(FixtureCache::default())),
            finalizer_cache: Rc::new(RefCell::new(FinalizerCache::default())),
            request_state: None,
            coverage,
            failed_count: 0,
        }
    }

    /// Returns whether failure count reached configured scheduling budget.
    fn max_fail_reached(&self) -> bool {
        self.context
            .settings()
            .test()
            .max_fail
            .is_exceeded_by(self.failed_count)
    }

    /// Adds one failed variant to `max-fail` accounting.
    fn record_outcome(&mut self, passed: bool) {
        if !passed {
            self.failed_count = self.failed_count.saturating_add(1);
        }
    }

    /// Registers a discovery or setup error against one test.
    fn register_error_test(&mut self, test: &DiscoveredTestFunction, error: TestError) {
        self.state.register_test_case_result(
            self.context,
            &QualifiedTestName::new(test.name().clone()),
            error.into_outcome(),
            std::time::Duration::ZERO,
            None,
        );
        self.record_outcome(false);
    }

    /// Registers a setup error against one concrete scheduled variant.
    fn register_error_variant(&mut self, variant: &TestVariant<'_>, error: TestError) {
        let name = variant.identity.as_ref().map_or_else(
            || QualifiedTestName::new(variant.test.name().clone()),
            |identity| {
                QualifiedTestName::with_parameters(
                    variant.test.name().clone(),
                    identity.display.as_ref().unwrap_or(&identity.node).clone(),
                )
            },
        );
        self.state.register_test_case_result(
            self.context,
            &name,
            error.into_outcome(),
            std::time::Duration::ZERO,
            None,
        );
        self.record_outcome(false);
    }

    /// Registers one shared module error against tests not blocked by `max-fail`.
    fn register_error_module_tests(&mut self, module: &DiscoveredModule, error: &TestError) {
        for test in module.test_functions() {
            self.register_error_test(test, error.clone());
            if self.max_fail_reached() {
                return;
            }
        }
    }

    /// Registers one shared package error throughout its remaining test tree.
    fn register_error_package_tests(&mut self, package: &DiscoveredPackage, error: &TestError) {
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
    fn validate_parametrization(&mut self, package: &DiscoveredPackage) -> bool {
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
        features: &mut CompilationFeatures,
    ) {
        let mut child_parents = parents.to_vec();
        child_parents.push(package);

        for module in package.modules().values() {
            for test in module.test_functions() {
                let compiler = FixturePlanCompiler::new(&child_parents, module, package.path());
                let plan = CompiledTestPlan::compile(py, test, compiler);
                if let Ok(plan) = &plan {
                    features.include(plan);
                }
                plans.insert(test.name().to_string(), plan);
            }
        }

        for child_package in package.packages().values() {
            Self::compile_test_plans(py, child_package, &child_parents, plans, features);
        }
    }

    fn add_request_items(
        py: Python<'_>,
        package: &DiscoveredPackage,
        plans: &CompiledTestPlans,
        state: &mut RequestState,
    ) -> PyResult<()> {
        for module in package.modules().values() {
            for test in module.test_functions() {
                let Some(Ok(plan)) = plans.get(&test.name().to_string()) else {
                    continue;
                };
                for parameter_id in plan.variant_ids() {
                    state.add_item(py, test, parameter_id.as_deref())?;
                }
            }
        }
        for child_package in package.packages().values() {
            Self::add_request_items(py, child_package, plans, state)?;
        }
        Ok(())
    }

    fn collect_scheduled_variants<'a>(
        &mut self,
        package: &'a DiscoveredPackage,
        packages: &mut Vec<&'a DiscoveredPackage>,
        plans: &mut CompiledTestPlans,
        modules: &mut Vec<ScheduledModule<'a>>,
        variants: &mut Vec<ScheduledVariant<'a>>,
    ) {
        packages.push(package);
        for module in package.modules().values() {
            let module_index = modules.len();
            modules.push(ScheduledModule {
                module,
                packages: packages.clone(),
            });
            for test in module.test_functions() {
                let Some(plan) = plans.remove(&test.name().to_string()) else {
                    continue;
                };
                let plan = match plan {
                    Ok(plan) => plan,
                    Err(error) => {
                        self.register_error_test(
                            test,
                            TestError::new(fixture_resolution_diagnostic(error)),
                        );
                        if self.max_fail_reached() {
                            let _ = packages.pop();
                            return;
                        }
                        continue;
                    }
                };
                variants.extend(TestVariantIterator::new(test, plan).map(|variant| {
                    ScheduledVariant {
                        module: module_index,
                        variant,
                    }
                }));
            }
        }
        if !self.max_fail_reached() {
            for child_package in package.packages().values() {
                self.collect_scheduled_variants(child_package, packages, plans, modules, variants);
                if self.max_fail_reached() {
                    break;
                }
            }
        }
        let _ = packages.pop();
    }

    fn execute_scheduled_variants(
        &mut self,
        py: Python<'_>,
        session: &DiscoveredPackage,
        plans: &mut CompiledTestPlans,
    ) {
        let mut modules = Vec::new();
        let mut scheduled = Vec::new();
        self.collect_scheduled_variants(
            session,
            &mut Vec::new(),
            plans,
            &mut modules,
            &mut scheduled,
        );
        if self.max_fail_reached() {
            return;
        }
        let order = reorder_variant_ref_indices(
            &scheduled
                .iter()
                .map(|item| &item.variant)
                .collect::<Vec<_>>(),
        );
        let reorder_error = self.request_state.as_ref().and_then(|state| {
            state.as_ref().as_ref().ok().and_then(|state| {
                state
                    .borrow()
                    .reorder_items(
                        py,
                        order
                            .iter()
                            .map(|index| scheduled[*index].variant.request_node_id()),
                    )
                    .err()
            })
        });
        if let Some(error) = reorder_error {
            self.request_state = Some(Box::new(Err(error.to_string())));
        }
        let mut scheduled = scheduled.into_iter().map(Some).collect::<Vec<_>>();
        let mut active_packages: Vec<&DiscoveredPackage> = Vec::new();
        let mut active_module = None;
        let mut failed_packages: HashMap<Utf8PathBuf, TestError> = HashMap::new();
        let mut failed_modules: HashMap<Utf8PathBuf, TestError> = HashMap::new();

        for index in order {
            let Some(item) = scheduled.get_mut(index).and_then(Option::take) else {
                continue;
            };
            let target = &modules[item.module];
            if active_module != Some(item.module) {
                if active_module.take().is_some() {
                    self.report_scope_cleanup(py, ScopeKey::Module);
                }

                let common = active_packages
                    .iter()
                    .zip(&target.packages)
                    .take_while(|(active, target)| active.path() == target.path())
                    .count();
                for package in active_packages[common..].iter().rev() {
                    self.report_scope_cleanup(py, ScopeKey::Package(package.path()));
                }
                active_packages.truncate(common);

                let mut package_error = None;
                for (package_index, package) in target.packages[common..].iter().enumerate() {
                    let absolute_index = common + package_index;
                    if let Some(error) = failed_packages.get(package.path()) {
                        package_error = Some(error.clone());
                        break;
                    }
                    if package.configuration_module_impl().is_some()
                        && let Err(error) = self.run_auto_use_fixtures(
                            py,
                            &target.packages[..absolute_index],
                            *package,
                            package.path(),
                            FixtureScope::Package,
                        )
                    {
                        failed_packages.insert(package.path().clone(), error.clone());
                        package_error = Some(error);
                        break;
                    }
                    active_packages.push(package);
                }
                if let Some(error) = package_error {
                    self.register_error_variant(&item.variant, error);
                    if self.max_fail_reached() {
                        break;
                    }
                    continue;
                }

                if let Some(error) = failed_modules.get(target.module.path()) {
                    self.register_error_variant(&item.variant, error.clone());
                    if self.max_fail_reached() {
                        break;
                    }
                    continue;
                }
                let package_path = target
                    .packages
                    .last()
                    .map_or_else(|| target.module.path(), |package| package.path());
                if let Err(error) = self.run_auto_use_fixtures(
                    py,
                    &target.packages,
                    target.module,
                    package_path,
                    FixtureScope::Module,
                ) {
                    failed_modules.insert(target.module.path().clone(), error.clone());
                    self.register_error_variant(&item.variant, error);
                    if self.max_fail_reached() {
                        break;
                    }
                    continue;
                }
                active_module = Some(item.module);
            }

            let passed = self.execute_test_variant(py, item.variant);
            self.record_outcome(passed);
            if self.max_fail_reached() {
                break;
            }
        }

        if active_module.is_some() {
            self.report_scope_cleanup(py, ScopeKey::Module);
        }
        for package in active_packages.iter().rev() {
            self.report_scope_cleanup(py, ScopeKey::Package(package.path()));
        }
    }

    /// Executes all discovered tests and session-scoped fixture teardown.
    pub(crate) fn execute(&mut self, py: Python<'_>, session: &DiscoveredPackage) {
        if !self.validate_parametrization(session) {
            return;
        }

        let mut test_plans = HashMap::new();
        let mut features = CompilationFeatures::default();
        Self::compile_test_plans(py, session, &[], &mut test_plans, &mut features);
        if features.uses_request {
            self.request_state = Some(Box::new(
                RequestState::new(
                    py,
                    self.context.cwd().as_str(),
                    self.context.is_verbose(),
                    self.context
                        .settings()
                        .max_fail()
                        .limit()
                        .map_or(0, std::num::NonZero::get),
                    &self.context.settings().test().test_function_prefix,
                    &self.context.settings().src().include_paths,
                )
                .and_then(|mut state| {
                    Self::add_request_items(py, session, &test_plans, &mut state)?;
                    Ok(RefCell::new(state))
                })
                .map_err(|error| error.to_string()),
            ));
        }

        if let Err(error) =
            self.run_auto_use_fixtures(py, &[], session, session.path(), FixtureScope::Session)
        {
            self.register_error_package_tests(session, &error);
            return;
        }

        if features.requires_cross_module_reordering
            || (features.uses_request && features.requires_reordering)
        {
            self.execute_scheduled_variants(py, session, &mut test_plans);
        } else {
            self.execute_package(py, session, &[], &mut test_plans);
        }
        self.report_scope_cleanup(py, ScopeKey::Session);
    }

    /// Executes module auto-use fixtures, variants, and module teardown.
    fn execute_module(
        &mut self,
        py: Python<'_>,
        module: &DiscoveredModule,
        parents: &[&DiscoveredPackage],
        test_plans: &mut CompiledTestPlans,
    ) -> bool {
        let package_path = parents
            .last()
            .map_or_else(|| module.path(), |package| package.path());
        if let Err(error) =
            self.run_auto_use_fixtures(py, parents, module, package_path, FixtureScope::Module)
        {
            self.register_error_module_tests(module, &error);
            return false;
        }

        let requires_reordering = module.test_functions().iter().any(|test| {
            test_plans
                .get(&test.name().to_string())
                .and_then(|plan| plan.as_ref().ok())
                .is_some_and(CompiledTestPlan::requires_reordering)
        });
        let mut scheduled_variants = Vec::new();
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

            if requires_reordering {
                scheduled_variants.extend(variants);
                continue;
            }

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

        if requires_reordering && !self.max_fail_reached() {
            for variant in reorder_variants(scheduled_variants) {
                let variant_passed = self.execute_test_variant(py, variant);
                self.record_outcome(variant_passed);
                passed &= variant_passed;

                if self.max_fail_reached() {
                    break;
                }
            }
        }

        self.report_scope_cleanup(py, ScopeKey::Module);
        passed
    }

    /// Recursively executes package modules, child packages, and teardown.
    fn execute_package(
        &mut self,
        py: Python<'_>,
        package: &DiscoveredPackage,
        parents: &[&DiscoveredPackage],
        test_plans: &mut CompiledTestPlans,
    ) -> bool {
        let mut child_parents = parents.to_vec();
        child_parents.push(package);

        if package.configuration_module_impl().is_some()
            && let Err(error) = self.run_auto_use_fixtures(
                py,
                parents,
                package,
                package.path(),
                FixtureScope::Package,
            )
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

        self.report_scope_cleanup(py, ScopeKey::Package(package.path()));
        passed
    }
}
