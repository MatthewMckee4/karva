//! Normalization of discovered tests.
//!
//! Normalization converts discovered tests and fixtures into a form that is easier to execute.
//! When tests depend on fixtures, we resolve the fixture dependency graph and determine
//! all the combinations of fixtures needed for each test.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use pyo3::prelude::*;

mod models;
mod utils;

pub use models::{NormalizedModule, NormalizedPackage, NormalizedTest};

use crate::discovery::{DiscoveredModule, DiscoveredPackage, TestFunction};
use crate::extensions::fixtures::{
    Fixture, FixtureScope, HasFixtures, NormalizedFixture, RequiresFixtures, UserDefinedFixture,
    get_auto_use_fixtures, get_builtin_fixture,
};
use crate::extensions::tags::parametrize::ParametrizationArgs;
use crate::normalize::utils::cartesian_product_arc;
use crate::utils::iter_with_ancestors;

#[derive(Default)]
pub struct Normalizer {
    fixture_cache: HashMap<String, Arc<[Arc<NormalizedFixture>]>>,
}

impl Normalizer {
    pub(crate) fn normalize(
        &mut self,
        py: Python,
        session: &DiscoveredPackage,
    ) -> NormalizedPackage {
        let session_auto_use_fixtures =
            self.get_normalized_auto_use_fixtures(py, FixtureScope::Session, &[], session);

        let mut normalized_package = self.normalize_package(py, session, &[]);
        normalized_package.extend_auto_use_fixtures(session_auto_use_fixtures);

        normalized_package
    }

    fn normalize_fixture(
        &mut self,
        py: Python,
        fixture: &Fixture,
        parents: &[&DiscoveredPackage],
        module: &DiscoveredModule,
    ) -> Arc<[Arc<NormalizedFixture>]> {
        let cache_key = fixture.name().to_string();

        if let Some(cached) = self.fixture_cache.get(&cache_key) {
            return Arc::clone(cached);
        }

        let required_fixtures: Vec<String> = fixture.required_fixtures(py);
        let dependent_fixtures =
            self.get_dependent_fixtures(py, Some(fixture), &required_fixtures, parents, module);

        let normalized_dependent_fixtures = if dependent_fixtures.is_empty() {
            vec![Arc::from(Vec::new().into_boxed_slice())]
        } else {
            cartesian_product_arc(&dependent_fixtures)
        };

        let fixture_name = fixture.name().clone();
        let fixture_scope = fixture.scope();
        let is_generator = fixture.is_generator();
        let py_function = fixture.function().clone();
        let stmt_function_def = Arc::clone(fixture.stmt_function_def());

        let result: Arc<[Arc<NormalizedFixture>]> = normalized_dependent_fixtures
            .into_iter()
            .map(|dependencies| {
                Arc::new(NormalizedFixture::UserDefined(UserDefinedFixture {
                    name: fixture_name.clone(),
                    dependencies: dependencies.to_vec(),
                    scope: fixture_scope,
                    is_generator,
                    py_function: py_function.clone(),
                    stmt_function_def: Arc::clone(&stmt_function_def),
                }))
            })
            .collect();

        self.fixture_cache.insert(cache_key, Arc::clone(&result));

        result
    }

    fn normalize_test_function(
        &mut self,
        py: Python<'_>,
        test_function: &TestFunction,
        parents: &[&DiscoveredPackage],
        module: &DiscoveredModule,
    ) -> Vec<NormalizedTest> {
        let test_params = test_function.tags.parametrize_args();

        let parametrize_param_names: HashSet<&str> = test_params
            .iter()
            .flat_map(|params| params.values().keys().map(String::as_str))
            .collect();

        let all_param_names = test_function.stmt_function_def.required_fixtures(py);
        let regular_fixture_names: Vec<String> = all_param_names
            .into_iter()
            .filter(|name| !parametrize_param_names.contains(name.as_str()))
            .collect();

        let function_auto_use_fixtures =
            self.get_normalized_auto_use_fixtures(py, FixtureScope::Function, parents, module);

        let dependent_fixtures =
            self.get_dependent_fixtures(py, None, &regular_fixture_names, parents, module);

        let use_fixture_names = test_function.tags.required_fixtures_names();
        let normalized_use_fixtures =
            self.get_dependent_fixtures(py, None, &use_fixture_names, parents, module);

        let test_params: Vec<ParametrizationArgs> = if test_params.is_empty() {
            vec![ParametrizationArgs::default()]
        } else {
            test_params
        };

        let dep_combinations = cartesian_product_arc(&dependent_fixtures);
        let use_fixture_combinations = cartesian_product_arc(&normalized_use_fixtures);
        let auto_use_fixtures: Arc<[Arc<NormalizedFixture>]> = function_auto_use_fixtures.into();

        let total_tests =
            dep_combinations.len() * use_fixture_combinations.len() * test_params.len();
        let mut result = Vec::with_capacity(total_tests);

        let test_name = test_function.name.clone();
        let test_py_function = test_function.py_function.clone();
        let test_stmt_function_def = Arc::clone(&test_function.stmt_function_def);
        let base_tags = &test_function.tags;

        for dep_combination in &dep_combinations {
            for use_fixture_combination in &use_fixture_combinations {
                for param_args in &test_params {
                    let mut new_tags = base_tags.clone();
                    new_tags.extend(&param_args.tags);

                    result.push(NormalizedTest {
                        name: test_name.clone(),
                        params: param_args.values.clone(),
                        fixture_dependencies: dep_combination.to_vec(),
                        use_fixture_dependencies: use_fixture_combination.to_vec(),
                        auto_use_fixtures: auto_use_fixtures.to_vec(),
                        function: test_py_function.clone(),
                        tags: new_tags,
                        stmt_function_def: Arc::clone(&test_stmt_function_def),
                    });
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
        let module_auto_use_fixtures =
            self.get_normalized_auto_use_fixtures(py, FixtureScope::Module, parents, module);

        let test_functions = module.test_functions();
        let mut normalized_test_functions = Vec::with_capacity(test_functions.len());

        for test_function in test_functions {
            let normalized_tests = self.normalize_test_function(py, test_function, parents, module);
            normalized_test_functions.extend(normalized_tests);
        }

        NormalizedModule {
            test_functions: normalized_test_functions,
            auto_use_fixtures: module_auto_use_fixtures,
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

        let package_modules = package.modules();
        let mut modules = HashMap::with_capacity(package_modules.len());

        for (path, module) in package_modules {
            let normalized_module = self.normalize_module(py, module, &new_parents);
            modules.insert(path.clone(), normalized_module);
        }

        let package_packages = package.packages();
        let mut packages = HashMap::with_capacity(package_packages.len());

        for (path, sub_package) in package_packages {
            let normalized_package = self.normalize_package(py, sub_package, &new_parents);
            packages.insert(path.clone(), normalized_package);
        }

        NormalizedPackage {
            modules,
            packages,
            auto_use_fixtures: package_auto_use_fixtures,
        }
    }

    fn get_normalized_auto_use_fixtures<'a>(
        &mut self,
        py: Python,
        scope: FixtureScope,
        parents: &'a [&'a DiscoveredPackage],
        current: &'a dyn HasFixtures<'a>,
    ) -> Vec<Arc<NormalizedFixture>> {
        let auto_use_fixtures = get_auto_use_fixtures(parents, current, scope);

        let Some(configuration_module) = current.configuration_module() else {
            return Vec::new();
        };

        let mut normalized_auto_use_fixtures = Vec::with_capacity(auto_use_fixtures.len());

        for fixture in auto_use_fixtures {
            let normalized = self.normalize_fixture(py, fixture, parents, configuration_module);
            normalized_auto_use_fixtures.extend(normalized.iter().cloned());
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
    ) -> Vec<Arc<[Arc<NormalizedFixture>]>> {
        let mut normalized_fixtures = Vec::with_capacity(fixture_names.len());

        for dep_name in fixture_names {
            if let Some(builtin_fixture) = get_builtin_fixture(py, dep_name) {
                let single: Arc<[Arc<NormalizedFixture>]> =
                    Arc::from(vec![Arc::new(builtin_fixture)].into_boxed_slice());
                normalized_fixtures.push(single);
            } else if let Some(fixture) = find_fixture(current_fixture, dep_name, parents, current)
            {
                let normalized = self.normalize_fixture(py, fixture, parents, current);
                normalized_fixtures.push(normalized);
            }
        }

        normalized_fixtures
    }
}

/// Finds a fixture by name, searching in the current module and parent packages.
/// We pass in the current fixture to avoid returning it (which would cause infinite recursion).
fn find_fixture<'a>(
    current_fixture: Option<&Fixture>,
    name: &str,
    parents: &'a [&'a DiscoveredPackage],
    current: &'a DiscoveredModule,
) -> Option<&'a Fixture> {
    if let Some(fixture) = current.get_fixture(name)
        && current_fixture.is_none_or(|current_fixture| current_fixture.name() != fixture.name())
    {
        return Some(fixture);
    }

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
