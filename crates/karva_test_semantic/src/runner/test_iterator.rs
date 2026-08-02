//! Expansion of one discovered test into executable parameter/fixture variants.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use pyo3::prelude::*;

use crate::discovery::DiscoveredTestFunction;
use crate::extensions::fixtures::{FixtureId, FixturePlan};
use crate::extensions::tags::parametrize::{
    ParameterPlan, ParameterPlanIterator, ParametrizationArgs, make_unique_parametrize_ids,
};
use crate::extensions::tags::{CompiledTags, RuntimeTags};
use crate::runner::fixture_resolver::{FixturePlanCompiler, FixtureResolutionResult};

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
    /// Reference to the original discovered test function. Borrowed from the
    /// surrounding module, which outlives the iterator.
    pub(super) test: &'a DiscoveredTestFunction,

    /// Parameter values for this variant (from @parametrize). Moved out of
    /// the owning `ParametrizationArgs` so that `Arc::try_unwrap` in the
    /// caller can unwrap without a Python refcount bump.
    pub(super) params: HashMap<String, Arc<Py<PyAny>>>,

    /// Parameter values consumed by fixture `request.param` objects.
    pub(super) fixture_params: HashMap<String, FixtureParameter>,

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
}

/// One selected fixture parameter for a concrete test variant.
#[derive(Clone, Debug)]
pub(super) struct FixtureParameter {
    pub(super) value: Arc<Py<PyAny>>,
    pub(super) index: usize,
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
    /// Discovered Python function shared by all emitted variants.
    test: &'a DiscoveredTestFunction,
    /// Consumed as we iterate, so `values` and `tags` on each
    /// `ParametrizationArgs` are moved into the emitted variant (not cloned).
    param_args: ParameterPlanIterator,
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
    fixture_plan: Rc<FixturePlan>,
    fixture_dependencies: Rc<[FixtureId]>,
    use_fixture_dependencies: Rc<[FixtureId]>,
    auto_use_fixtures: Rc<[FixtureId]>,
    parameters: ParameterPlan,
    runtime_tags: RuntimeTags,
}

impl CompiledTestPlan {
    /// Resolves all fixture roots for one test without executing fixture code.
    pub(super) fn compile(
        py: Python,
        test: &DiscoveredTestFunction,
        mut compiler: FixturePlanCompiler<'_>,
    ) -> FixtureResolutionResult<Self> {
        let tags = CompiledTags::new(&test.tags);
        let parametrize_param_names = tags.parameter_names();

        let auto_use_fixtures = compiler.get_normalized_auto_use_fixtures(
            py,
            crate::extensions::fixtures::FixtureScope::Function,
        )?;

        let fixture_dependencies =
            compiler.resolve_test_fixtures(py, test, &parametrize_param_names)?;

        let use_fixture_dependencies =
            compiler.resolve_use_fixtures(py, tags.required_fixtures())?;
        let (mut parameters, runtime_tags) = tags.into_runtime();

        compiler.compile_dynamic_fixtures(py);
        let fixture_plan = Rc::new(compiler.finish());
        let indirectly_parametrized = parameters
            .indirect_names()
            .filter_map(|name| fixture_plan.dynamic_fixture(name))
            .collect::<std::collections::HashSet<_>>();
        for (fixture_id, fixture) in fixture_plan.variant_fixtures() {
            if indirectly_parametrized.contains(&fixture_id) {
                continue;
            }
            let Some(fixture_parameters) = fixture.parameters() else {
                continue;
            };
            let name = fixture.name().to_string();
            let mut dimension = fixture_parameters
                .iter()
                .enumerate()
                .map(|(index, parameter)| {
                    ParametrizationArgs::fixture(name.clone(), parameter, index)
                })
                .collect::<Vec<_>>();
            make_unique_parametrize_ids(&mut dimension);
            parameters.push_dimension(dimension);
        }

        Ok(Self {
            fixture_plan,
            fixture_dependencies: Rc::from(fixture_dependencies),
            use_fixture_dependencies: Rc::from(use_fixture_dependencies),
            auto_use_fixtures: Rc::from(auto_use_fixtures),
            parameters,
            runtime_tags,
        })
    }
}

impl<'a> TestVariantIterator<'a> {
    /// Consumes one compiled plan into concrete test variants.
    pub(super) fn new(test: &'a DiscoveredTestFunction, plan: CompiledTestPlan) -> Self {
        Self {
            test,
            param_args: plan.parameters.into_iter(),
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
        let param_args = self.param_args.next()?;
        let id = param_args.id().map(str::to_string);

        let mut params = param_args.values;
        let mut fixture_params = HashMap::new();
        for indirect_name in &param_args.indirect {
            let fixture_name = self
                .fixture_plan
                .dynamic_fixture(indirect_name)
                .map(|fixture_id| self.fixture_plan.fixture(fixture_id).name().to_string())
                .unwrap_or_else(|| indirect_name.clone());
            if let Some(value) = params.remove(indirect_name) {
                fixture_params.insert(
                    fixture_name,
                    FixtureParameter {
                        value,
                        index: param_args
                            .indices
                            .get(indirect_name)
                            .copied()
                            .unwrap_or_default(),
                    },
                );
            }
        }

        let mut tags = self.runtime_tags.clone();
        tags.extend(&param_args.tags);

        Some(TestVariant {
            test: self.test,
            id,
            params,
            fixture_params,
            fixture_plan: Rc::clone(&self.fixture_plan),
            fixture_dependencies: Rc::clone(&self.fixture_dependencies),
            use_fixture_dependencies: Rc::clone(&self.use_fixture_dependencies),
            auto_use_fixtures: Rc::clone(&self.auto_use_fixtures),
            tags,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.param_args.size_hint()
    }
}
