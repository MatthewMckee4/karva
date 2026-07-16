use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use pyo3::prelude::*;

use crate::discovery::DiscoveredTestFunction;
use crate::extensions::fixtures::{NormalizedFixture, RequiresFixtures};
use crate::extensions::tags::Tags;
use crate::extensions::tags::parametrize::ParametrizationArgs;
use crate::runner::fixture_resolver::{FixtureCycleError, RuntimeFixtureResolver};

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
    pub test: &'a DiscoveredTestFunction,

    /// Parameter values for this variant (from @parametrize). Moved out of
    /// the owning `ParametrizationArgs` so that `Arc::try_unwrap` in the
    /// caller can unwrap without a Python refcount bump.
    pub params: HashMap<String, Arc<Py<PyAny>>>,

    /// Fixtures to be passed as arguments to the test function.
    pub fixture_dependencies: Rc<[Rc<NormalizedFixture>]>,

    /// Fixtures from @usefixtures (run for side effects, not passed as args).
    pub use_fixture_dependencies: Rc<[Rc<NormalizedFixture>]>,

    /// Auto-use fixtures that run automatically before this test.
    pub auto_use_fixtures: Rc<[Rc<NormalizedFixture>]>,

    /// Combined tags from the test and its parameter set.
    pub tags: Tags,
}

impl TestVariant<'_> {
    /// Get the module path for diagnostics.
    pub(super) fn module_path(&self) -> &camino::Utf8PathBuf {
        self.test.name.module_path().path()
    }

    /// Get the resolved tags including those from fixture dependencies.
    pub(super) fn resolved_tags(&self) -> Tags {
        let mut tags = self.tags.clone();

        for dependency in self.fixture_dependencies.iter() {
            tags.extend(&dependency.resolved_tags());
        }

        for dependency in self.use_fixture_dependencies.iter() {
            tags.extend(&dependency.resolved_tags());
        }

        for dependency in self.auto_use_fixtures.iter() {
            tags.extend(&dependency.resolved_tags());
        }

        tags
    }
}

/// Iterates over all variants of a test function.
///
/// Expands parametrize combinations to produce all concrete test invocations.
/// The iterator borrows the underlying `DiscoveredTestFunction` from the
/// module and shares fixture lists between variants via `Rc<[…]>`, so
/// producing N variants costs N refcount bumps rather than N deep clones.
pub(super) struct TestVariantIterator<'a> {
    test: &'a DiscoveredTestFunction,
    /// Consumed as we iterate, so `values` and `tags` on each
    /// `ParametrizationArgs` are moved into the emitted variant (not cloned).
    param_args: std::vec::IntoIter<ParametrizationArgs>,
    fixture_dependencies: Rc<[Rc<NormalizedFixture>]>,
    use_fixture_dependencies: Rc<[Rc<NormalizedFixture>]>,
    auto_use_fixtures: Rc<[Rc<NormalizedFixture>]>,
}

impl<'a> TestVariantIterator<'a> {
    /// Create a new iterator for the given test function.
    ///
    /// Resolves fixtures and computes all parametrize variants.
    pub(super) fn new(
        py: Python,
        test: &'a DiscoveredTestFunction,
        resolver: &mut RuntimeFixtureResolver,
    ) -> Result<Self, FixtureCycleError> {
        let test_params = test.tags.parametrize_args();

        let parametrize_param_names: HashSet<&str> = test_params
            .iter()
            .flat_map(|params| params.values().keys().map(String::as_str))
            .collect();

        // Only use the function parameter names, NOT the use_fixtures names.
        // use_fixtures are run for side effects but not passed as arguments.
        let function_param_names = test.stmt_function_def.required_fixtures(py);

        let auto_use_fixtures = resolver.get_normalized_auto_use_fixtures(
            py,
            crate::extensions::fixtures::FixtureScope::Function,
        )?;

        let fixture_dependencies =
            resolver.resolve_test_fixtures(py, &function_param_names, &parametrize_param_names)?;

        let use_fixture_names = test.tags.required_fixtures_names();
        let use_fixture_dependencies = resolver.resolve_use_fixtures(py, &use_fixture_names)?;

        let param_args = if test_params.is_empty() {
            vec![ParametrizationArgs::default()]
        } else {
            test_params
        };

        Ok(Self {
            test,
            param_args: param_args.into_iter(),
            fixture_dependencies: Rc::from(fixture_dependencies),
            use_fixture_dependencies: Rc::from(use_fixture_dependencies),
            auto_use_fixtures: Rc::from(auto_use_fixtures),
        })
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
            params: param_args.values,
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
