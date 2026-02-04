//! Normalization of discovered tests.
//!
//! Normalization converts discovered tests and fixtures into a form that is easier to execute.
//! When tests depend on fixtures, we resolve the fixture dependency graph and determine
//! all the combinations of fixtures needed for each test.
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

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
use crate::normalize::utils::cartesian_product_rc;
use crate::utils::iter_with_ancestors;

/// Transforms discovered tests and fixtures into normalized, executable form.
///
/// Resolves fixture dependencies, expands parametrized tests, and produces
/// concrete test instances ready for execution.
#[derive(Default)]
pub struct Normalizer {
    /// Cache of normalized fixtures to avoid redundant processing.
    fixture_cache: HashMap<String, Rc<[Rc<NormalizedFixture>]>>,
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
    ) -> Rc<[Rc<NormalizedFixture>]> {
        let cache_key = fixture.name().to_string();

        if let Some(cached) = self.fixture_cache.get(&cache_key) {
            return Rc::clone(cached);
        }

        let required_fixtures: Vec<String> = fixture.required_fixtures(py);
        let dependent_fixtures =
            self.get_dependent_fixtures(py, Some(fixture), &required_fixtures, parents, module);

        let normalized_dependent_fixtures = if dependent_fixtures.is_empty() {
            vec![Rc::from(Vec::new().into_boxed_slice())]
        } else {
            cartesian_product_rc(&dependent_fixtures)
        };

        let fixture_name = fixture.name().clone();
        let fixture_scope = fixture.scope();
        let is_generator = fixture.is_generator();
        let stmt_function_def = Rc::clone(fixture.stmt_function_def());

        let result: Rc<[Rc<NormalizedFixture>]> = normalized_dependent_fixtures
            .into_iter()
            .map(|dependencies| {
                Rc::new(NormalizedFixture::UserDefined(UserDefinedFixture {
                    name: fixture_name.clone(),
                    dependencies: dependencies.to_vec(),
                    scope: fixture_scope,
                    is_generator,
                    py_function: Rc::new(fixture.function().clone_ref(py)),
                    stmt_function_def: Rc::clone(&stmt_function_def),
                }))
            })
            .collect();

        self.fixture_cache.insert(cache_key, Rc::clone(&result));

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

        let dep_combinations = cartesian_product_rc(&dependent_fixtures);
        let use_fixture_combinations = cartesian_product_rc(&normalized_use_fixtures);
        let auto_use_fixtures: Rc<[Rc<NormalizedFixture>]> = function_auto_use_fixtures.into();

        let total_tests =
            dep_combinations.len() * use_fixture_combinations.len() * test_params.len();
        let mut result = Vec::with_capacity(total_tests);

        let test_name = test_function.name.clone();
        let test_py_function = test_function.py_function.clone_ref(py);
        let test_stmt_function_def = Rc::clone(&test_function.stmt_function_def);
        let base_tags = &test_function.tags;

        for dep_combination in &dep_combinations {
            for use_fixture_combination in &use_fixture_combinations {
                for param_args in &test_params {
                    let mut new_tags = base_tags.clone();
                    new_tags.extend(&param_args.tags);

                    let params: HashMap<String, Py<PyAny>> = param_args
                        .values
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone_ref(py)))
                        .collect();
                    result.push(NormalizedTest {
                        name: test_name.clone(),
                        params,
                        fixture_dependencies: dep_combination.to_vec(),
                        use_fixture_dependencies: use_fixture_combination.to_vec(),
                        auto_use_fixtures: auto_use_fixtures.to_vec(),
                        function: test_py_function.clone_ref(py),
                        tags: new_tags,
                        stmt_function_def: Rc::clone(&test_stmt_function_def),
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

        let normalized_test_functions = module
            .test_functions()
            .iter()
            .flat_map(|test_function| {
                self.normalize_test_function(py, test_function, parents, module)
            })
            .collect();

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

        let modules = package
            .modules()
            .values()
            .map(|module| self.normalize_module(py, module, &new_parents))
            .collect();

        let packages = package
            .packages()
            .values()
            .map(|sub_package| self.normalize_package(py, sub_package, &new_parents))
            .collect();

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
    ) -> Vec<Rc<NormalizedFixture>> {
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
    ) -> Vec<Rc<[Rc<NormalizedFixture>]>> {
        let mut normalized_fixtures = Vec::with_capacity(fixture_names.len());

        for dep_name in fixture_names {
            if let Some(builtin_fixture) = get_builtin_fixture(py, dep_name) {
                let single: Rc<[Rc<NormalizedFixture>]> =
                    Rc::from(vec![Rc::new(builtin_fixture)].into_boxed_slice());
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
