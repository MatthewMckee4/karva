//! Expansion of one discovered test into executable parameter/fixture variants.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use pyo3::prelude::*;

use crate::discovery::DiscoveredTestFunction;
use crate::extensions::fixtures::{FixtureId, FixturePlan};
use crate::extensions::tags::Tags;
use crate::extensions::tags::parametrize::ParametrizationArgs;
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
    pub(super) tags: Tags,
}

impl TestVariant<'_> {
    /// Get the module path for diagnostics.
    pub(super) fn module_path(&self) -> &camino::Utf8PathBuf {
        self.test.name().module_path().path()
    }

    /// Get the resolved tags including those from fixture dependencies.
    pub(super) fn resolved_tags(&self) -> Tags {
        self.tags.clone()
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
    param_args: std::vec::IntoIter<ParametrizationArgs>,
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
    param_args: Vec<ParametrizationArgs>,
}

impl CompiledTestPlan {
    /// Resolves all fixture roots for one test without executing fixture code.
    pub(super) fn compile(
        py: Python,
        test: &DiscoveredTestFunction,
        mut compiler: FixturePlanCompiler<'_>,
    ) -> FixtureResolutionResult<Self> {
        let parametrize_param_names = test.tags.parametrize_names();

        let auto_use_fixtures = compiler.get_normalized_auto_use_fixtures(
            py,
            crate::extensions::fixtures::FixtureScope::Function,
        )?;

        let fixture_dependencies =
            compiler.resolve_test_fixtures(py, test, &parametrize_param_names)?;

        let use_fixture_names = test.tags.required_fixtures_names();
        let use_fixture_dependencies = compiler.resolve_use_fixtures(py, &use_fixture_names)?;

        let test_params = test.tags.parametrize_args();
        let param_args = if test_params.is_empty() {
            vec![ParametrizationArgs::default()]
        } else {
            test_params
        };

        Ok(Self {
            fixture_plan: Rc::new(compiler.finish()),
            fixture_dependencies: Rc::from(fixture_dependencies),
            use_fixture_dependencies: Rc::from(use_fixture_dependencies),
            auto_use_fixtures: Rc::from(auto_use_fixtures),
            param_args,
        })
    }
}

impl<'a> TestVariantIterator<'a> {
    /// Consumes one compiled plan into concrete test variants.
    pub(super) fn new(test: &'a DiscoveredTestFunction, plan: CompiledTestPlan) -> Self {
        Self {
            test,
            param_args: plan.param_args.into_iter(),
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

        let mut tags = self.test.tags.clone();
        tags.extend(&param_args.tags);

        Some(TestVariant {
            test: self.test,
            id: param_args.id().map(str::to_string),
            params: param_args.values,
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

impl ExactSizeIterator for TestVariantIterator<'_> {}
