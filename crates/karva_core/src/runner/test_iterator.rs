use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use pyo3::prelude::*;

use crate::discovery::DiscoveredTestFunction;
use crate::extensions::fixtures::{NormalizedFixture, RequiresFixtures};
use crate::extensions::tags::Tags;
use crate::extensions::tags::parametrize::ParametrizationArgs;
use crate::runner::fixture_resolver::RuntimeFixtureResolver;

/// A single variant of a test to be executed.
///
/// Represents one specific invocation of a test function with:
/// - A specific set of parametrize values
/// - Resolved fixture dependencies
/// - Combined tags from the test and parameter set
pub(super) struct TestVariant {
    /// Reference to the original discovered test function.
    pub test: Rc<DiscoveredTestFunction>,

    /// Parameter values for this variant (from @parametrize).
    pub params: HashMap<String, Arc<Py<PyAny>>>,

    /// Fixtures to be passed as arguments to the test function.
    pub fixture_dependencies: Vec<Rc<NormalizedFixture>>,

    /// Fixtures from @usefixtures (run for side effects, not passed as args).
    pub use_fixture_dependencies: Vec<Rc<NormalizedFixture>>,

    /// Auto-use fixtures that run automatically before this test.
    pub auto_use_fixtures: Vec<Rc<NormalizedFixture>>,

    /// Combined tags from the test and its parameter set.
    pub tags: Tags,
}

impl TestVariant {
    /// Get the module path for diagnostics.
    pub(super) fn module_path(&self) -> &camino::Utf8PathBuf {
        self.test.name.module_path().path()
    }

    /// Get the resolved tags including those from fixture dependencies.
    pub(super) fn resolved_tags(&self) -> Tags {
        let mut tags = self.tags.clone();

        for dependency in &self.fixture_dependencies {
            tags.extend(&dependency.resolved_tags());
        }

        for dependency in &self.use_fixture_dependencies {
            tags.extend(&dependency.resolved_tags());
        }

        for dependency in &self.auto_use_fixtures {
            tags.extend(&dependency.resolved_tags());
        }

        tags
    }
}

/// Iterates over all variants of a test function.
///
/// Expands parametrize combinations to produce all concrete test invocations.
pub(super) struct TestVariantIterator {
    test: Rc<DiscoveredTestFunction>,
    param_args: Vec<ParametrizationArgs>,
    fixture_dependencies: Vec<Rc<NormalizedFixture>>,
    use_fixture_dependencies: Vec<Rc<NormalizedFixture>>,
    auto_use_fixtures: Vec<Rc<NormalizedFixture>>,

    param_index: usize,
}

impl TestVariantIterator {
    /// Create a new iterator for the given test function.
    ///
    /// Resolves fixtures and computes all parametrize variants.
    pub(super) fn new(
        py: Python,
        test: &DiscoveredTestFunction,
        resolver: &mut RuntimeFixtureResolver,
    ) -> Self {
        let test_params = test.tags.parametrize_args();

        let parametrize_param_names: HashSet<&str> = test_params
            .iter()
            .flat_map(|params| params.values().keys().map(String::as_str))
            .collect();

        // Only use the function parameter names, NOT the use_fixtures names.
        // use_fixtures are run for side effects but not passed as arguments.
        let function_param_names = test.stmt_function_def.required_fixtures(py);

        let function_auto_use_fixtures = resolver.get_normalized_auto_use_fixtures(
            py,
            crate::extensions::fixtures::FixtureScope::Function,
        );

        let fixture_dependencies =
            resolver.resolve_test_fixtures(py, &function_param_names, &parametrize_param_names);

        let use_fixture_names = test.tags.required_fixtures_names();
        let use_fixture_dependencies = resolver.resolve_use_fixtures(py, &use_fixture_names);

        let param_args: Vec<ParametrizationArgs> = if test_params.is_empty() {
            vec![ParametrizationArgs::default()]
        } else {
            test_params
        };

        Self {
            test: Rc::new(DiscoveredTestFunction {
                name: test.name.clone(),
                stmt_function_def: Rc::clone(&test.stmt_function_def),
                py_function: test.py_function.clone_ref(py),
                tags: test.tags.clone(),
            }),
            param_args,
            fixture_dependencies,
            use_fixture_dependencies,
            auto_use_fixtures: function_auto_use_fixtures,
            param_index: 0,
        }
    }
}

impl Iterator for TestVariantIterator {
    type Item = TestVariant;

    fn next(&mut self) -> Option<Self::Item> {
        if self.param_index >= self.param_args.len() {
            return None;
        }

        let param_args = &self.param_args[self.param_index];

        let mut new_tags = self.test.tags.clone();
        new_tags.extend(&param_args.tags);

        let variant = TestVariant {
            test: Rc::clone(&self.test),
            params: param_args.values.clone(),
            fixture_dependencies: self.fixture_dependencies.clone(),
            use_fixture_dependencies: self.use_fixture_dependencies.clone(),
            auto_use_fixtures: self.auto_use_fixtures.clone(),
            tags: new_tags,
        };

        self.param_index += 1;

        Some(variant)
    }
}
