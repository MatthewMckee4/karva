//! Expansion of one discovered test into executable parameter/fixture variants.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use karva_diagnostic::Diagnostic;
use karva_python_semantic::ParameterCase;
use pyo3::prelude::*;

use crate::diagnostic::fixture_resolution_diagnostic;
use crate::discovery::DiscoveredTestFunction;
use crate::extensions::fixtures::{FixtureDependency, FixtureId, FixturePlan};
use crate::extensions::tags::parametrize::{
    FixtureParameter, ParameterPlan, ParameterPlanIterator,
};
use crate::extensions::tags::{CompiledTags, RuntimeTags};
use crate::runner::fixture_resolver::FixturePlanCompiler;

/// A single variant of a test to be executed.
///
/// Represents one specific invocation of a test function with:
/// - A specific set of parametrize values
/// - Resolved fixture dependencies
/// - Combined tags from the test and parameter set
///
/// The fixture lists are shared between every variant of a test via `Rc<[…]>`,
/// so producing a new variant is a handful of refcount bumps rather than a
/// full `Vec` clone per fixture set.
pub(super) struct TestVariant<'a> {
    /// Deferred until variant filtering has selected this test.
    pub(super) fixture_error: Option<Box<Diagnostic>>,

    /// Reference to the original discovered test function. Borrowed from the
    /// surrounding module, which outlives the iterator.
    pub(super) test: &'a DiscoveredTestFunction,

    /// Parameter values for this variant (from @parametrize). Moved out of
    /// the owning `ParametrizationArgs` so that `Arc::try_unwrap` in the
    /// caller can unwrap without a Python refcount bump.
    pub(super) params: HashMap<String, Arc<Py<PyAny>>>,
    pub(super) indirect: HashMap<String, FixtureParameter>,

    pub id: Option<String>,

    /// Arena shared by all fixture root groups for this test.
    pub(super) fixture_plan: Rc<FixturePlan>,

    /// Fixtures to be passed as arguments to the test function.
    pub(super) fixture_dependencies: Rc<[FixtureId]>,

    /// Fixtures from @usefixtures (run for side effects, not passed as args).
    pub(super) use_fixture_dependencies: Rc<[FixtureId]>,

    /// Auto-use fixtures that run automatically before this test.
    pub(super) auto_use_fixtures: Rc<[FixtureId]>,

    /// Combined tags from the test and its parameter set.
    pub(super) tags: RuntimeTags,

    /// Original parametrize index in the test's full case list (`None` for
    /// non-parametrized tests). Used to form a stable per-case cache key
    /// even when the worker only ran a filtered subset of cases.
    pub(super) case: Option<ParameterCase>,
    pub(super) request_id: Option<String>,
    pub(super) case_markers: Vec<Arc<Py<PyAny>>>,
}

impl TestVariant<'_> {
    /// Get the module path for diagnostics.
    pub(super) fn module_path(&self) -> &camino::Utf8PathBuf {
        self.test.name().module_path().path()
    }
}

/// Iterates over all variants of a test function.
///
/// Expands parametrize combinations to produce all concrete test invocations.
/// The iterator borrows the underlying `DiscoveredTestFunction` from the
/// module and shares fixture lists between variants via `Rc<[…]>`, so
/// producing N variants costs N refcount bumps rather than N deep clones.
pub(super) struct TestVariantIterator<'a> {
    /// Deferred until variant filtering has selected this test.
    fixture_error: Option<Box<Diagnostic>>,

    /// Discovered Python function shared by all emitted variants.
    test: &'a DiscoveredTestFunction,
    /// Consumed as we iterate, so parameter values and tags move into variants.
    param_args: ParameterPlanIterator,
    /// Selected parameter indices in controller scheduling order.
    case_filter: Option<std::vec::IntoIter<usize>>,
    pending_cases: std::ops::Range<usize>,
    fixture_case_count: usize,
    /// Index of the next item in the full parametrize expansion.
    next_case_index: usize,
    /// Whether emitted variants need a stable parametrize index.
    parametrized: bool,
    grouped_cases: bool,
    /// Runtime policy shared by every parameter variant.
    runtime_tags: RuntimeTags,
    /// Resolved fixtures passed as test arguments.
    fixture_plan: Rc<FixturePlan>,
    fixture_dependencies: Rc<[FixtureId]>,
    /// Resolved side-effect-only fixtures.
    use_fixture_dependencies: Rc<[FixtureId]>,
    /// Resolved function-scoped auto-use fixtures.
    auto_use_fixtures: Rc<[FixtureId]>,
}

/// Fixture roots and parameter cases compiled before any test fixture executes.
pub(super) struct CompiledTestPlan {
    /// Deferred until variant filtering has selected this test.
    fixture_error: Option<Box<Diagnostic>>,

    fixture_plan: Rc<FixturePlan>,
    fixture_dependencies: Rc<[FixtureId]>,
    use_fixture_dependencies: Rc<[FixtureId]>,
    auto_use_fixtures: Rc<[FixtureId]>,
    parameters: ParameterPlan,
    fixture_case_count: usize,
    has_fixture_params: bool,
    runtime_tags: RuntimeTags,
}

/// Per-test roots and parameter cases compiled against a module fixture arena.
pub(super) struct PendingTestPlan {
    /// Deferred until variant filtering has selected this test.
    fixture_error: Option<Box<Diagnostic>>,

    fixture_dependencies: Rc<[FixtureId]>,
    use_fixture_dependencies: Rc<[FixtureId]>,
    auto_use_fixtures: Rc<[FixtureId]>,
    parameters: ParameterPlan,
    fixture_case_count: usize,
    has_fixture_params: bool,
    runtime_tags: RuntimeTags,
}

impl PendingTestPlan {
    /// Resolves one test's fixture roots without executing fixture code.
    pub(super) fn compile(
        py: Python,
        test: &DiscoveredTestFunction,
        compiler: &mut FixturePlanCompiler<'_>,
    ) -> Self {
        let mut tags = CompiledTags::new(&test.tags);
        let parameter_names = tags.take_parameter_names();

        let uses_request =
            test.required_fixtures().contains(&"request") || !tags.indirect_names().is_empty();
        if uses_request {
            compiler.require_request();
        }

        let indirect_names = tags.indirect_names().clone();
        let fixtures =
            compiler.with_parameters(parameter_names, indirect_names.clone(), |compiler| {
                let auto_use = compiler.get_normalized_auto_use_fixtures(
                    py,
                    crate::extensions::fixtures::FixtureScope::Function,
                )?;
                let dependencies = compiler.resolve_test_fixtures(py, test)?;
                let mut use_dependencies =
                    compiler.resolve_root_fixtures(py, tags.required_fixtures())?;
                for id in compiler.resolve_indirect_fixtures(py, test, &indirect_names)? {
                    if !dependencies.contains(&id) && !use_dependencies.contains(&id) {
                        use_dependencies.push(id);
                    }
                }
                compiler.retain_dynamic_parameters(test.name());
                Ok((dependencies, use_dependencies, auto_use))
            });
        let (fixture_dependencies, use_fixture_dependencies, auto_use_fixtures, fixture_error) =
            match fixtures {
                Ok((dependencies, use_dependencies, auto_use)) => {
                    (dependencies, use_dependencies, auto_use, None)
                }
                Err(error) => (
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                    Some(Box::new(fixture_resolution_diagnostic(error))),
                ),
            };
        let mut fixture_case_count = 1usize;
        let mut has_fixture_params = false;
        let mut names = test
            .tags
            .parametrize_names()
            .map(str::to_owned)
            .chain(indirect_names)
            .collect::<std::collections::HashSet<_>>();
        let (mut parameters, runtime_tags) = tags.into_runtime();
        parameters.set_indirect_scopes(|name| compiler.root_fixture_scope(name));
        let mut pending = auto_use_fixtures
            .iter()
            .chain(&use_fixture_dependencies)
            .chain(&fixture_dependencies)
            .copied()
            .collect::<Vec<_>>();
        pending.reverse();
        let mut visited = std::collections::HashSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let fixture = compiler.fixture(id);
            pending.extend(
                fixture
                    .dependencies()
                    .iter()
                    .rev()
                    .filter_map(FixtureDependency::fixture_id),
            );
            if let Some(cases) = &fixture.parameters
                && names.insert(fixture.function_name().to_owned())
            {
                has_fixture_params = true;
                let radix = parameters.push_fixture(cases);
                fixture_case_count = fixture_case_count.saturating_mul(radix);
            }
        }

        Self {
            fixture_error,
            fixture_dependencies: Rc::from(fixture_dependencies),
            use_fixture_dependencies: Rc::from(use_fixture_dependencies),
            auto_use_fixtures: Rc::from(auto_use_fixtures),
            parameters,
            fixture_case_count,
            has_fixture_params,
            runtime_tags,
        }
    }

    /// Attaches the immutable arena shared by every test in the module.
    pub(super) fn finish(self, fixture_plan: Rc<FixturePlan>) -> CompiledTestPlan {
        CompiledTestPlan {
            fixture_error: self.fixture_error,
            fixture_plan,
            fixture_dependencies: self.fixture_dependencies,
            use_fixture_dependencies: self.use_fixture_dependencies,
            auto_use_fixtures: self.auto_use_fixtures,
            parameters: self.parameters,
            fixture_case_count: self.fixture_case_count,
            has_fixture_params: self.has_fixture_params,
            runtime_tags: self.runtime_tags,
        }
    }
}

impl<'a> TestVariantIterator<'a> {
    /// Consumes one compiled plan into concrete test variants.
    pub(super) fn new(test: &'a DiscoveredTestFunction, plan: CompiledTestPlan) -> Self {
        Self {
            fixture_error: plan.fixture_error,
            test,
            param_args: plan.parameters.into_iter(),
            case_filter: test.case_filter.clone().map(Vec::into_iter),
            pending_cases: 0..0,
            fixture_case_count: plan.fixture_case_count,
            next_case_index: 0,
            parametrized: test.tags.has_parametrize() || plan.has_fixture_params,
            grouped_cases: test.tags.has_parametrize() && plan.has_fixture_params,
            runtime_tags: plan.runtime_tags,
            fixture_plan: plan.fixture_plan,
            fixture_dependencies: plan.fixture_dependencies,
            use_fixture_dependencies: plan.use_fixture_dependencies,
            auto_use_fixtures: plan.auto_use_fixtures,
        }
    }
}

impl<'a> Iterator for TestVariantIterator<'a> {
    type Item = TestVariant<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let (case_index, param_args) = if self.case_filter.is_some() {
            if self.pending_cases.is_empty() {
                let index = self.case_filter.as_mut()?.next()?;
                self.pending_cases = self.case_range(index);
            }
            let index = self.pending_cases.next()?;
            (Some(index), self.param_args.seek(index)?)
        } else if !self.parametrized {
            (None, self.param_args.next()?)
        } else {
            let case_index = self.next_case_index;
            self.next_case_index += 1;
            (Some(case_index), self.param_args.next()?)
        };

        Some(self.make_variant(case_index, param_args))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.param_args.size_hint()
    }
}

impl<'a> TestVariantIterator<'a> {
    pub(super) fn case_range(&self, index: usize) -> std::ops::Range<usize> {
        let start = index.saturating_mul(self.fixture_case_count);
        start..start.saturating_add(self.fixture_case_count)
    }

    pub(super) fn next_case(&mut self, index: usize) -> Option<TestVariant<'a>> {
        let param_args = self.param_args.seek(index)?;
        Some(self.make_variant(Some(index), param_args))
    }

    fn make_variant(
        &self,
        case_index: Option<usize>,
        param_args: crate::extensions::tags::parametrize::ParametrizationArgs,
    ) -> TestVariant<'a> {
        let mut tags = self.runtime_tags.clone();
        tags.extend(&param_args.tags);
        let request_id = self.parametrized.then(|| param_args.case_id().to_owned());

        TestVariant {
            fixture_error: self.fixture_error.clone(),
            test: self.test,
            id: param_args.id().map(str::to_string),
            params: param_args.values,
            indirect: param_args.indirect,
            fixture_plan: Rc::clone(&self.fixture_plan),
            fixture_dependencies: Rc::clone(&self.fixture_dependencies),
            use_fixture_dependencies: Rc::clone(&self.use_fixture_dependencies),
            auto_use_fixtures: Rc::clone(&self.auto_use_fixtures),
            tags,
            case: case_index.map(|index| {
                if self.grouped_cases {
                    ParameterCase::Fixture([
                        index / self.fixture_case_count,
                        index % self.fixture_case_count,
                    ])
                } else {
                    ParameterCase::Index(index)
                }
            }),
            request_id,
            case_markers: param_args.tags.into_pytest_marks(),
        }
    }
}
