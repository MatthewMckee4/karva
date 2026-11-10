//! Expansion of one discovered test into executable parameter/fixture variants.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use karva_diagnostic::Diagnostic;
use pyo3::prelude::*;

use crate::diagnostic::fixture_resolution_diagnostic;
use crate::discovery::DiscoveredTestFunction;
use crate::extensions::fixtures::{FixtureId, FixturePlan};
use crate::extensions::tags::parametrize::{ParameterPlan, ParameterPlanIterator};
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
    pub(super) case_index: Option<usize>,
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
    /// Index of the next item in the full parametrize expansion.
    next_case_index: usize,
    /// Whether emitted variants need a stable parametrize index.
    parametrized: bool,
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
    runtime_tags: RuntimeTags,
}

impl PendingTestPlan {
    /// Resolves one test's fixture roots without executing fixture code.
    pub(super) fn compile(
        py: Python,
        test: &DiscoveredTestFunction,
        compiler: &mut FixturePlanCompiler<'_>,
    ) -> Self {
        let tags = CompiledTags::new(&test.tags);
        let parametrize_param_names = tags.parameter_names();

        let fixtures = (|| {
            let auto_use = compiler.get_normalized_auto_use_fixtures(
                py,
                crate::extensions::fixtures::FixtureScope::Function,
            )?;
            let dependencies =
                compiler.resolve_test_fixtures(py, test, &parametrize_param_names)?;
            let use_dependencies = compiler.resolve_use_fixtures(py, tags.required_fixtures())?;
            Ok((dependencies, use_dependencies, auto_use))
        })();
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
        let (parameters, runtime_tags) = tags.into_runtime();

        Self {
            fixture_error,
            fixture_dependencies: Rc::from(fixture_dependencies),
            use_fixture_dependencies: Rc::from(use_fixture_dependencies),
            auto_use_fixtures: Rc::from(auto_use_fixtures),
            parameters,
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
            next_case_index: 0,
            parametrized: test.tags.has_parametrize(),
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
        let (case_index, param_args) = if let Some(indices) = &mut self.case_filter {
            let index = indices.next()?;
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

        TestVariant {
            fixture_error: self.fixture_error.clone(),
            test: self.test,
            id: param_args.id().map(str::to_string),
            params: param_args.values,
            fixture_plan: Rc::clone(&self.fixture_plan),
            fixture_dependencies: Rc::clone(&self.fixture_dependencies),
            use_fixture_dependencies: Rc::clone(&self.use_fixture_dependencies),
            auto_use_fixtures: Rc::clone(&self.auto_use_fixtures),
            tags,
            case_index,
        }
    }
}
