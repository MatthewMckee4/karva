//! Package-tree orchestration and run-wide execution state.

use camino::Utf8PathBuf;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use karva_coverage::CoverageSession;
use karva_python_semantic::QualifiedTestName;
use pyo3::prelude::*;

use crate::diagnostic::{fixture_resolution_diagnostic, invalid_parametrize_diagnostic};
use crate::discovery::{DiscoveredModule, DiscoveredPackage, DiscoveredTestFunction};
use crate::extensions::fixtures::FixtureScope;
use crate::runner::fixture_resolver::{FixturePlanCompiler, FixtureResolutionError};
use crate::runner::scoped_storage::ScopeKey;
use crate::runner::test_iterator::{CompiledTestPlan, PendingTestPlan, TestVariantIterator};
use crate::runner::{FinalizerCache, FixtureCache};
use crate::{Context, RunState};

mod failure;
mod fixture;
mod outcome;
mod variant;

pub use fixture::{FixtureCallError, FixtureChainEntry};

use failure::TestError;

type TestPlanKey = (Utf8PathBuf, String);
type CompiledTestPlans = HashMap<TestPlanKey, Result<CompiledTestPlan, FixtureResolutionError>>;
type VariantIterators<'a> = HashMap<TestPlanKey, TestVariantIterator<'a>>;

struct TestContext<'a> {
    package: &'a DiscoveredPackage,
    parents: Vec<&'a DiscoveredPackage>,
    module: &'a DiscoveredModule,
    test: &'a DiscoveredTestFunction,
    case_index: Option<usize>,
}

/// Tracks selected-test counts and start order for scope cleanup.
struct ScopeLifetimeState {
    module_remaining: HashMap<Utf8PathBuf, usize>,
    package_remaining: HashMap<Utf8PathBuf, usize>,
    started_modules: HashSet<Utf8PathBuf>,
    started_packages: HashSet<Utf8PathBuf>,
    module_start_order: Vec<Utf8PathBuf>,
    package_start_order: Vec<Utf8PathBuf>,
}

impl ScopeLifetimeState {
    fn new(contexts: &[TestContext<'_>]) -> Self {
        let mut state = Self {
            module_remaining: HashMap::new(),
            package_remaining: HashMap::new(),
            started_modules: HashSet::new(),
            started_packages: HashSet::new(),
            module_start_order: Vec::new(),
            package_start_order: Vec::new(),
        };
        for context in contexts {
            *state
                .module_remaining
                .entry(context.module.path().clone())
                .or_default() += 1;
            let mut chain = context.parents.clone();
            chain.push(context.package);
            for package in chain {
                *state
                    .package_remaining
                    .entry(package.path().clone())
                    .or_default() += 1;
            }
        }
        state
    }
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
    fixture_cache: FixtureCache,
    /// Fixture finalizers retained until their declared scope completes.
    finalizer_cache: FinalizerCache,
    /// Active coverage session for this worker, when coverage is enabled.
    coverage: Option<&'context CoverageSession>,
    /// Failed variants observed so far, used to enforce `max-fail`.
    failed_count: u32,
    /// Module whose scoped fixtures are currently being prepared or executed.
    active_module: Option<Utf8PathBuf>,
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
            fixture_cache: FixtureCache::default(),
            finalizer_cache: FinalizerCache::default(),
            coverage,
            failed_count: 0,
            active_module: None,
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
        self.context.register_test_case_result(
            &QualifiedTestName::new(test.name().clone()),
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
                let Some(statement) = test.function_statement() else {
                    continue;
                };
                if let Err(error) = test.tags.validate_parametrize(statement) {
                    let diagnostic = invalid_parametrize_diagnostic(
                        test.source_file().clone(),
                        statement,
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
            let mut compiler = FixturePlanCompiler::new(&child_parents, module, package.path());
            let mut module_plans = Vec::with_capacity(module.test_functions().len());

            for test in module.test_functions() {
                module_plans.push((
                    test.name().to_string(),
                    PendingTestPlan::compile(py, test, &mut compiler),
                ));
            }

            let fixture_plan = Rc::new(compiler.finish());
            for (name, plan) in module_plans {
                plans.insert(
                    (module.path().clone(), name),
                    plan.map(|plan| plan.finish(Rc::clone(&fixture_plan))),
                );
            }
        }

        for child_package in package.packages().values() {
            Self::compile_test_plans(py, child_package, &child_parents, plans);
        }
    }

    /// Executes all discovered tests and session-scoped fixture teardown.
    pub(crate) fn execute(&mut self, py: Python<'_>, session: &DiscoveredPackage) {
        if !self.validate_parametrization(session) {
            return;
        }

        let mut test_plans = HashMap::new();
        Self::compile_test_plans(py, session, &[], &mut test_plans);

        if let Err(error) =
            self.run_auto_use_fixtures(py, &[], session, session.path(), FixtureScope::Session)
        {
            self.register_error_package_tests(session, &error);
            return;
        }

        self.execute_ordered(py, session, &mut test_plans);
        self.report_scope_cleanup(py, ScopeKey::Session);
    }

    fn collect_test_contexts<'a>(
        package: &'a DiscoveredPackage,
        parents: &[&'a DiscoveredPackage],
        contexts: &mut Vec<TestContext<'a>>,
    ) {
        let mut child_parents = parents.to_vec();
        child_parents.push(package);
        for module in package.modules().values() {
            for test in module.test_functions() {
                contexts.push(TestContext {
                    package,
                    parents: parents.to_vec(),
                    module,
                    test,
                    case_index: None,
                });
            }
        }
        for child in package.packages().values() {
            Self::collect_test_contexts(child, &child_parents, contexts);
        }
    }

    fn ordered_test_contexts(session: &DiscoveredPackage) -> Vec<TestContext<'_>> {
        let mut discovered = Vec::new();
        Self::collect_test_contexts(session, &[], &mut discovered);
        let order = session.test_order();
        if order.is_empty() {
            return discovered;
        }
        let context_by_key = discovered
            .iter()
            .enumerate()
            .map(|(index, context)| {
                (
                    (
                        context.module.path().clone(),
                        context.test.name().function_name().to_owned(),
                    ),
                    index,
                )
            })
            .collect::<HashMap<_, _>>();
        let mut ordered = Vec::with_capacity(order.len());
        for (path, name, case_index) in order {
            if let Some(&index) = context_by_key.get(&(path.clone(), name.clone())) {
                let context = &discovered[index];
                ordered.push(TestContext {
                    package: context.package,
                    parents: context.parents.clone(),
                    module: context.module,
                    test: context.test,
                    case_index: *case_index,
                });
            }
        }
        ordered
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "test execution receives independent plan and cache state"
    )]
    fn execute_one_test<'a>(
        &mut self,
        py: Python<'_>,
        module: &DiscoveredModule,
        test: &'a DiscoveredTestFunction,
        test_plans: &mut CompiledTestPlans,
        variant_iterators: &mut VariantIterators<'a>,
        failed_plans: &mut HashMap<TestPlanKey, TestError>,
        case_index: Option<usize>,
    ) -> bool {
        let key = (module.path().clone(), test.name().to_string());
        if !variant_iterators.contains_key(&key) && !failed_plans.contains_key(&key) {
            let Some(test_plan) = test_plans.remove(&key) else {
                return false;
            };
            match test_plan {
                Ok(plan) => {
                    variant_iterators.insert(key.clone(), TestVariantIterator::new(test, plan));
                }
                Err(error) => {
                    failed_plans.insert(
                        key.clone(),
                        TestError::new(fixture_resolution_diagnostic(error)),
                    );
                }
            }
        }
        if let Some(error) = failed_plans.get(&key) {
            self.register_error_test(test, error.clone());
            return false;
        }
        let Some(iterator) = variant_iterators.get_mut(&key) else {
            return false;
        };
        let variant = if let Some(index) = case_index {
            iterator.next_case(index)
        } else {
            iterator.next()
        };
        let Some(variant) = variant else {
            return false;
        };
        let mut passed = self.execute_test_variant(py, variant);
        self.record_outcome(passed);
        if case_index.is_some() {
            return passed;
        }

        for variant in iterator {
            let variant_passed = self.execute_test_variant(py, variant);
            self.record_outcome(variant_passed);
            passed &= variant_passed;
            if self.max_fail_reached() {
                break;
            }
        }
        passed
    }

    fn cleanup_finished_scopes(
        &mut self,
        py: Python<'_>,
        context: &TestContext<'_>,
        scopes: &mut ScopeLifetimeState,
    ) {
        if let Some(remaining) = scopes.module_remaining.get_mut(context.module.path()) {
            *remaining = remaining.saturating_sub(1);
            if *remaining == 0 && scopes.started_modules.remove(context.module.path()) {
                self.report_scope_cleanup(py, ScopeKey::Module(context.module.path()));
                self.active_module = None;
                scopes
                    .module_start_order
                    .retain(|path| path != context.module.path());
            }
        }
        let mut package_chain = context.parents.clone();
        package_chain.push(context.package);
        for package in package_chain.into_iter().rev() {
            if let Some(remaining) = scopes.package_remaining.get_mut(package.path()) {
                *remaining = remaining.saturating_sub(1);
                if *remaining == 0 && scopes.started_packages.remove(package.path()) {
                    self.report_scope_cleanup(py, ScopeKey::Package(package.path()));
                    scopes
                        .package_start_order
                        .retain(|path| path != package.path());
                }
            }
        }
    }

    fn cleanup_all_started_scopes(&mut self, py: Python<'_>, scopes: &mut ScopeLifetimeState) {
        for module in scopes.module_start_order.drain(..).rev() {
            self.report_scope_cleanup(py, ScopeKey::Module(&module));
        }
        scopes.started_modules.clear();
        for package in scopes.package_start_order.drain(..).rev() {
            self.report_scope_cleanup(py, ScopeKey::Package(&package));
        }
        scopes.started_packages.clear();
        self.active_module = None;
    }

    /// Executes the selected tests in scheduler order, preserving each fixture scope.
    fn execute_ordered(
        &mut self,
        py: Python<'_>,
        session: &DiscoveredPackage,
        test_plans: &mut CompiledTestPlans,
    ) -> bool {
        let contexts = if self.context.settings().test().failed_first {
            Self::ordered_test_contexts(session)
        } else {
            let mut contexts = Vec::new();
            Self::collect_test_contexts(session, &[], &mut contexts);
            contexts
        };
        let mut scopes = ScopeLifetimeState::new(&contexts);
        let mut failed_modules = HashMap::<Utf8PathBuf, TestError>::new();
        let mut failed_packages = HashMap::<Utf8PathBuf, TestError>::new();
        let mut variant_iterators = VariantIterators::new();
        let mut failed_plans = HashMap::new();
        let mut passed = true;

        for context in &contexts {
            let mut package_error = None;
            let mut package_chain = context.parents.clone();
            package_chain.push(context.package);
            for (index, package) in package_chain.iter().enumerate() {
                if let Some(error) = failed_packages.get(package.path()) {
                    package_error = Some(error.clone());
                    break;
                }
                if scopes.started_packages.insert(package.path().clone()) {
                    scopes.package_start_order.push(package.path().clone());
                    let result = self.run_auto_use_fixtures(
                        py,
                        &package_chain[..index],
                        *package,
                        package.path(),
                        FixtureScope::Package,
                    );
                    if let Err(error) = result {
                        failed_packages.insert(package.path().clone(), error.clone());
                        package_error = Some(error);
                        break;
                    }
                }
            }

            let test_passed = if let Some(error) = package_error {
                self.register_error_test(context.test, error);
                false
            } else if let Some(error) = failed_modules.get(context.module.path()) {
                self.register_error_test(context.test, error.clone());
                false
            } else {
                self.active_module = Some(context.module.path().clone());
                if scopes.started_modules.insert(context.module.path().clone()) {
                    scopes
                        .module_start_order
                        .push(context.module.path().clone());
                    let mut module_parents = context.parents.clone();
                    module_parents.push(context.package);
                    if let Err(error) = self.run_auto_use_fixtures(
                        py,
                        &module_parents,
                        context.module,
                        context.package.path(),
                        FixtureScope::Module,
                    ) {
                        failed_modules.insert(context.module.path().clone(), error.clone());
                        self.register_error_test(context.test, error);
                        false
                    } else {
                        self.execute_one_test(
                            py,
                            context.module,
                            context.test,
                            test_plans,
                            &mut variant_iterators,
                            &mut failed_plans,
                            context.case_index,
                        )
                    }
                } else {
                    self.execute_one_test(
                        py,
                        context.module,
                        context.test,
                        test_plans,
                        &mut variant_iterators,
                        &mut failed_plans,
                        context.case_index,
                    )
                }
            };
            passed &= test_passed;
            self.cleanup_finished_scopes(py, context, &mut scopes);
            if self.max_fail_reached() {
                break;
            }
        }
        self.cleanup_all_started_scopes(py, &mut scopes);
        passed
    }
}
