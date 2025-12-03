//! Normalization of discovered tests.
//!
//! ## What is normalization?
//! There is one main reason we need to "normalize" tests.
//!
//! When tests depend on fixtures that are parameterized, like the following:
//! ```python
//! from karva import fixture
//!
//! @karva.fixture(params=["a", "b"])
//! def fixture_function(request):
//!     return request.param
//!
//! def test_function(fixture_function):
//!     ...
//! ```
//!
//! We are in a weird situation when we come to resolve fixtures for `test_function`.
//!
//! We need to know about the number of parameters for the fixture,
//! so that we can first generate all combinations of parameters for the function,
//! and run them while respecting auto fixtures and finalizers.
//!
//! If we got all of the fixture values at the start, we would not be able to run auto use fixtures
//! and finalizers in a predictable way.
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use pyo3::prelude::*;

mod models;
mod utils;

pub use models::{NormalizedModule, NormalizedPackage, NormalizedTest};

use crate::{
    discovery::{DiscoveredModule, DiscoveredPackage, TestFunction},
    extensions::{
        fixtures::{
            Fixture, FixtureScope, HasFixtures, NormalizedFixture, RequiresFixtures,
            UserDefinedFixture, get_auto_use_fixtures, get_builtin_fixture,
        },
        tags::{Parametrization, parametrize::ParametrizationArgs},
    },
    normalize::utils::cartesian_product,
    utils::iter_with_ancestors,
};

/// Use the `Normalizer` to turn discovered test functions into a more normalized form.
///
/// This struct converts discovered test functions into a form which is much easier to "run".
#[derive(Default)]
pub struct Normalizer {
    /// Cache for fixtures.
    ///
    /// Normalizing fixtures can sometimes be expensive, so we cache the results.
    fixture_cache: HashMap<String, Arc<Vec<NormalizedFixture>>>,
}

impl Normalizer {
    /// Normalize a `DiscoveredPackage` into a `NormalizedPackage`.
    pub(crate) fn normalize(
        &mut self,
        py: Python,
        session: &DiscoveredPackage,
    ) -> NormalizedPackage {
        let session_auto_use_fixtures =
            self.get_normalized_auto_use_fixtures(py, FixtureScope::Session, &[], &session);

        let mut normalized_package = self.normalize_package(py, session, &[]);

        normalized_package.extend_auto_use_fixtures(session_auto_use_fixtures);

        normalized_package
    }

    /// Normalizes a single fixture, handling parametrization and dependencies.
    /// Returns a Vec of `NormalizedFixture`, one for each parameter value.
    fn normalize_fixture(
        &mut self,
        py: Python,
        fixture: &Fixture,
        parents: &[&DiscoveredPackage],
        module: &DiscoveredModule,
    ) -> Vec<NormalizedFixture> {
        let cache_key = fixture.name().to_string();

        if let Some(cached) = self.fixture_cache.get(&cache_key) {
            return (**cached).clone();
        }

        // Get all required fixtures (dependencies)
        // Filter out "request" as it's a special parameter, not a fixture dependency
        let required_fixtures: Vec<String> = fixture
            .required_fixtures(py)
            .into_iter()
            .filter(|name| name != "request")
            .collect();

        let dependent_fixtures =
            self.get_dependent_fixtures(py, Some(fixture), &required_fixtures, parents, module);

        let params = fixture.params().cloned().unwrap_or_default();

        let mut result = Vec::new();

        // If there are no parameters, use a single None value
        let param_list: Vec<Option<Parametrization>> = if params.is_empty() {
            vec![None]
        } else {
            params.into_iter().map(Some).collect()
        };

        let normalized_dependent_fixtures = if dependent_fixtures.is_empty() {
            vec![vec![]]
        } else {
            cartesian_product(dependent_fixtures)
        };

        for dependent_fixtures in normalized_dependent_fixtures {
            let dependent_fixtures = Arc::new(dependent_fixtures);
            for param in &param_list {
                let normalized = NormalizedFixture::UserDefined(UserDefinedFixture {
                    name: fixture.name().clone(),
                    param: param.clone(),
                    dependencies: dependent_fixtures.clone(),
                    scope: fixture.scope(),
                    is_generator: fixture.is_generator(),
                    py_function: fixture.function().clone(),
                    stmt_function_def: fixture.stmt_function_def().clone(),
                });

                result.push(normalized);
            }
        }

        let result_arc = Arc::new(result);

        self.fixture_cache
            .insert(cache_key, Arc::clone(&result_arc));

        Arc::try_unwrap(result_arc).unwrap_or_else(|arc| (*arc).clone())
    }

    /// Normalizes a test function, handling parametrization and fixture dependencies.
    /// Returns a Vec of `NormalizedTestFunction`, one for each parameter combination.
    fn normalize_test_function(
        &mut self,
        py: Python<'_>,
        test_function: &TestFunction,
        parents: &[&DiscoveredPackage],
        module: &DiscoveredModule,
    ) -> Vec<NormalizedTest> {
        let function_auto_use_fixtures =
            self.get_normalized_auto_use_fixtures(py, FixtureScope::Function, parents, module);

        let test_params = test_function.tags.parametrize_args();

        let parametrize_param_names: HashSet<String> = test_params
            .iter()
            .flat_map(|params| params.values().keys().cloned())
            .collect();

        let all_param_names = test_function.stmt_function_def.required_fixtures(py);
        let regular_fixture_names: Vec<String> = all_param_names
            .into_iter()
            .filter(|name| !parametrize_param_names.contains(name))
            .collect();

        let use_fixture_names = test_function.tags.required_fixtures_names();

        let dependent_fixtures =
            self.get_dependent_fixtures(py, None, &regular_fixture_names, parents, module);

        // Normalize use_fixtures
        let mut normalized_use_fixtures: Vec<Vec<NormalizedFixture>> = Vec::new();

        for dep_name in &use_fixture_names {
            // Check for builtin fixtures first
            if let Some(builtin_fixture) =
                crate::extensions::fixtures::get_builtin_fixture(py, dep_name)
            {
                normalized_use_fixtures.push(vec![builtin_fixture]);
            } else if let Some(fixture) = find_fixture(None, dep_name, parents, module) {
                let normalized = self.normalize_fixture(py, fixture, parents, module);
                if !normalized.is_empty() {
                    normalized_use_fixtures.push(normalized);
                }
            }
        }

        // Ensure at least one test case exists (no parametrization)
        let test_params = if test_params.is_empty() {
            vec![ParametrizationArgs::default()]
        } else {
            test_params
        };

        let mut result = Vec::new();

        let dep_combinations = cartesian_product(dependent_fixtures);

        let use_fixture_combinations = cartesian_product(normalized_use_fixtures);

        let auto_use_fixtures_arc = Arc::new(function_auto_use_fixtures);

        for dep_combination in dep_combinations {
            let dep_combination_arc = Arc::new(dep_combination);
            for use_fixture_combination in &use_fixture_combinations {
                let use_fixture_combination_arc = Arc::new(use_fixture_combination.clone());
                for ParametrizationArgs {
                    values: parametrize_values,
                    tags,
                } in test_params.clone()
                {
                    let mut new_tags = test_function.tags.clone();
                    new_tags.extend(&tags);

                    let normalized = NormalizedTest {
                        name: test_function.name.clone(),
                        params: parametrize_values,
                        fixture_dependencies: Arc::clone(&dep_combination_arc),
                        use_fixture_dependencies: Arc::clone(&use_fixture_combination_arc),
                        auto_use_fixtures: Arc::clone(&auto_use_fixtures_arc),
                        function: test_function.py_function.clone(),
                        tags: new_tags,
                        stmt_function_def: test_function.stmt_function_def.clone(),
                    };

                    result.push(normalized);
                }
            }
        }

        result
    }

    fn normalize_module(
        &mut self,
        py: Python<'_>,
        module: &DiscoveredModule,
        parents: &[&DiscoveredPackage],
    ) -> NormalizedModule {
        tracing::debug!("Normalizing file: {}", module.path());

        let module_auto_use_fixtures =
            self.get_normalized_auto_use_fixtures(py, FixtureScope::Module, parents, module);

        let mut normalized_test_functions = Vec::new();

        for test_function in module.test_functions() {
            let normalized_tests = self.normalize_test_function(py, test_function, parents, module);

            normalized_test_functions.extend(normalized_tests);
        }

        NormalizedModule {
            test_functions: normalized_test_functions,
            auto_use_fixtures: Arc::new(module_auto_use_fixtures),
        }
    }

    fn normalize_package(
        &mut self,
        py: Python<'_>,
        package: &DiscoveredPackage,
        parents: &[&DiscoveredPackage],
    ) -> NormalizedPackage {
        let mut new_parents = parents.to_vec();

        new_parents.push(package);

        let package_auto_use_fixtures =
            self.get_normalized_auto_use_fixtures(py, FixtureScope::Package, parents, package);

        let mut modules = HashMap::new();

        for (path, module) in package.modules() {
            let normalized_module = self.normalize_module(py, module, &new_parents);
            modules.insert(path.clone(), normalized_module);
        }

        let mut packages = HashMap::new();

        for (path, sub_package) in package.packages() {
            let normalized_package = self.normalize_package(py, sub_package, &new_parents);
            packages.insert(path.clone(), normalized_package);
        }

        NormalizedPackage {
            modules,
            packages,
            auto_use_fixtures: Arc::new(package_auto_use_fixtures),
        }
    }

    fn get_normalized_auto_use_fixtures<'a>(
        &mut self,
        py: Python,
        scope: FixtureScope,
        parents: &'a [&'a DiscoveredPackage],
        current: &'a dyn HasFixtures<'a>,
    ) -> Vec<NormalizedFixture> {
        let auto_use_fixtures = get_auto_use_fixtures(parents, current, scope);

        let mut normalized_auto_use_fixtures = Vec::new();

        let Some(configuration_module) = current.configuration_module() else {
            return normalized_auto_use_fixtures;
        };

        for fixture in auto_use_fixtures {
            let normalized_fixture =
                self.normalize_fixture(py, fixture, parents, configuration_module);
            normalized_auto_use_fixtures.extend(normalized_fixture);
        }

        normalized_auto_use_fixtures
    }

    fn get_dependent_fixtures<'a>(
        &mut self,
        py: Python,
        current_fixture: Option<&Fixture>,
        fixture_names: &[String],
        parents: &'a [&'a DiscoveredPackage],
        current: &'a DiscoveredModule,
    ) -> Vec<Vec<NormalizedFixture>> {
        let mut normalized_fixtures = Vec::new();

        for dep_name in fixture_names {
            if let Some(builtin_fixture) = get_builtin_fixture(py, dep_name) {
                normalized_fixtures.push(vec![builtin_fixture]);
            } else if let Some(fixture) = find_fixture(current_fixture, dep_name, parents, current)
            {
                let normalized = self.normalize_fixture(py, fixture, parents, current);
                normalized_fixtures.push(normalized);
            }
        }

        normalized_fixtures
    }
}

/// Finds a fixture by name, searching in the current module and parent packages
///
/// We pass in the current fixture to ensure that we don't return the same fixture twice.
/// This can cause a stack overflow.
fn find_fixture<'a>(
    current_fixture: Option<&Fixture>,
    name: &str,
    parents: &'a [&'a DiscoveredPackage],
    current: &'a DiscoveredModule,
) -> Option<&'a Fixture> {
    // First check the current module
    if let Some(fixture) = current.get_fixture(name)
        && current_fixture.is_none_or(|current_fixture| current_fixture.name() != fixture.name())
    {
        return Some(fixture);
    }

    // Then check parent packages
    for (parent, _ancestors) in iter_with_ancestors(parents) {
        if let Some(fixture) = parent.get_fixture(name)
            && current_fixture
                .is_none_or(|current_fixture| current_fixture.name() != fixture.name())
        {
            return Some(fixture);
        }
    }

    None
}
