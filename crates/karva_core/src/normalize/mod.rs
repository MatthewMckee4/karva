use std::collections::HashMap;

use pyo3::prelude::*;

use crate::{
    Context,
    diagnostic::{Diagnostic, FunctionKind},
    discovery::{DiscoveredModule, DiscoveredPackage, TestFunction},
    extensions::fixtures::{
        Fixture, FixtureManager, HasFixtures, NormalizedFixture, RequiresFixtures,
    },
    normalize::{
        models::{NormalizedModule, NormalizedPackage, NormalizedTestFunction},
        utils::{cartesian_product, stringify_param, stringify_params},
    },
    utils::{function_definition_location, iter_with_ancestors},
};

pub mod models;
mod utils;

pub struct DiscoveredPackageNormalizer<'ctx, 'proj, 'rep> {
    context: &'ctx mut Context<'proj, 'rep>,
    /// Cache to avoid re-normalizing the same fixture multiple times
    /// Key: (`fixture_name`, `sorted_dependency_names`)
    normalization_cache: HashMap<String, Vec<NormalizedFixture>>,
}

impl<'ctx, 'proj, 'rep> DiscoveredPackageNormalizer<'ctx, 'proj, 'rep> {
    pub(crate) fn new(context: &'ctx mut Context<'proj, 'rep>) -> Self {
        Self {
            context,
            normalization_cache: HashMap::new(),
        }
    }

    pub(crate) fn normalize(
        &mut self,
        py: Python,
        package: DiscoveredPackage,
    ) -> NormalizedPackage {
        tracing::info!("Normalizing package");

        let mut fixture_manager = FixtureManager::new();

        // Normalize the package recursively
        self.normalize_package_impl(py, &package, &[], &mut fixture_manager)
    }

    /// Normalizes a single fixture, handling parametrization and dependencies.
    /// Returns a Vec of `NormalizedFixture`, one for each parameter value.
    fn normalize_fixture(
        &mut self,
        py: Python<'_>,
        fixture: &Fixture,
        parents: &[&DiscoveredPackage],
        current: &DiscoveredModule,
    ) -> Vec<NormalizedFixture> {
        // Check cache first
        let cache_key = fixture.name().to_string();
        if let Some(cached) = self.normalization_cache.get(&cache_key) {
            return cached.clone();
        }

        // Get all required fixtures (dependencies)
        // Filter out "request" as it's a special parameter, not a fixture dependency
        let dependency_names: Vec<String> = fixture
            .required_fixtures(py)
            .into_iter()
            .filter(|name| name != "request")
            .collect();

        // Recursively normalize each dependency
        let mut normalized_deps: Vec<Vec<NormalizedFixture>> = Vec::new();

        let mut missing_fixtures = Vec::new();

        for dep_name in &dependency_names {
            // Check for builtin fixtures first
            if let Some(builtin_fixture) =
                crate::extensions::fixtures::builtins::get_builtin_fixture(py, dep_name)
            {
                normalized_deps.push(vec![builtin_fixture]);
            } else if let Some(dep_fixture) = self.find_fixture(dep_name, parents, current) {
                let normalized = self.normalize_fixture(py, dep_fixture, parents, current);
                normalized_deps.push(normalized);
            } else {
                missing_fixtures.push(dep_name.clone());
            }
        }

        // Get fixture parameters
        let params = fixture.params().cloned().unwrap_or_default();

        // If no parameters and all dependencies have single variants, no expansion needed
        if params.is_empty() && normalized_deps.iter().all(|deps| deps.len() == 1) {
            let dependencies = normalized_deps
                .into_iter()
                .filter_map(|mut deps| deps.pop())
                .collect();

            let location = function_definition_location(current, fixture.function_definition());

            let normalized = NormalizedFixture::new(
                fixture.name().to_string(),
                Some(fixture.name().clone()),
                None,
                dependencies,
                location,
                missing_fixtures,
                fixture.scope(),
                fixture.auto_use(),
                fixture.is_generator(),
                fixture.function().clone(),
                Some(fixture.function_definition().clone()),
            );

            let result = vec![normalized];
            self.normalization_cache.insert(cache_key, result.clone());
            return result;
        }

        // Create cartesian product of dependencies and parameters
        let mut result = Vec::new();

        // If there are no parameters, use a single None value
        let param_list: Vec<Option<&Py<PyAny>>> = if params.is_empty() {
            vec![None]
        } else {
            params.iter().map(Some).collect()
        };

        // Get all dependency combinations
        let dep_combinations = if normalized_deps.is_empty() {
            vec![vec![]]
        } else {
            cartesian_product(normalized_deps)
        };

        // Compute location once for all variants
        let location = function_definition_location(current, fixture.function_definition());

        for dep_combination in dep_combinations {
            for param in &param_list {
                let name = if let Some(p) = param {
                    format!(
                        "{}[{}]",
                        fixture.name().function_name(),
                        stringify_param(py, p)
                    )
                } else {
                    fixture.name().function_name().to_string()
                };

                let normalized = NormalizedFixture::new(
                    name,
                    Some(fixture.name().clone()),
                    param.cloned(),
                    dep_combination.clone(),
                    location.clone(),
                    missing_fixtures.clone(),
                    fixture.scope(),
                    fixture.auto_use(),
                    fixture.is_generator(),
                    fixture.function().clone(),
                    Some(fixture.function_definition().clone()),
                );

                result.push(normalized);
            }
        }

        // Cache the result
        self.normalization_cache.insert(cache_key, result.clone());
        result
    }

    /// Normalizes a test function, handling parametrization and fixture dependencies.
    /// Returns a Vec of `NormalizedTestFunction`, one for each parameter combination.
    fn normalize_test_function(
        &mut self,
        py: Python<'_>,
        test_fn: &TestFunction,
        parents: &[&DiscoveredPackage],
        module: &DiscoveredModule,
    ) -> Vec<NormalizedTestFunction> {
        // Compute qualified name and location once
        let location = function_definition_location(module, test_fn.definition());

        // Get test parametrization (from @pytest.mark.parametrize)
        let test_params = test_fn.tags().parametrize_args();

        // Get parameter names from parametrize to exclude from fixtures
        let parametrize_param_names: Vec<String> = test_params
            .iter()
            .flat_map(|params| params.keys().cloned())
            .collect();

        // Get regular fixtures (from function parameters, excluding parametrize params)
        let all_param_names = test_fn.definition().required_fixtures(py);
        let regular_fixture_names: Vec<String> = all_param_names
            .into_iter()
            .filter(|name| !parametrize_param_names.contains(name))
            .collect();

        // Get use_fixtures (from tags - should only be executed, not passed as args)
        let use_fixture_names = test_fn.tags().required_fixtures_names();

        // Normalize regular fixtures
        let mut normalized_deps: Vec<Vec<NormalizedFixture>> = Vec::new();
        let mut missing_fixtures = Vec::new();

        for dep_name in &regular_fixture_names {
            // Check for builtin fixtures first
            if let Some(builtin_fixture) =
                crate::extensions::fixtures::builtins::get_builtin_fixture(py, dep_name)
            {
                normalized_deps.push(vec![builtin_fixture]);
            } else if let Some(fixture) = self.find_fixture(dep_name, parents, module) {
                let normalized = self.normalize_fixture(py, fixture, parents, module);
                normalized_deps.push(normalized);
            } else {
                missing_fixtures.push(dep_name.clone());
            }
        }

        // Normalize use_fixtures
        let mut normalized_use_fixtures: Vec<Vec<NormalizedFixture>> = Vec::new();

        for dep_name in &use_fixture_names {
            // Check for builtin fixtures first
            if let Some(builtin_fixture) =
                crate::extensions::fixtures::builtins::get_builtin_fixture(py, dep_name)
            {
                normalized_use_fixtures.push(vec![builtin_fixture]);
            } else if let Some(fixture) = self.find_fixture(dep_name, parents, module) {
                let normalized = self.normalize_fixture(py, fixture, parents, module);
                if !normalized.is_empty() {
                    normalized_use_fixtures.push(normalized);
                }
                // Note: we don't add missing use_fixtures to missing_fixtures
                // because they're optional - if they don't exist, we just don't use them
            }
        }

        // Ensure at least one test case exists (no parametrization)
        let test_params = if test_params.is_empty() {
            vec![HashMap::new()]
        } else {
            test_params
        };

        // If no parametrization needed, create single normalized test
        if test_params.len() == 1
            && test_params[0].is_empty()
            && normalized_deps.iter().all(|deps| deps.len() == 1)
            && normalized_use_fixtures.iter().all(|deps| deps.len() == 1)
        {
            let fixture_dependencies = normalized_deps
                .into_iter()
                .filter_map(|mut deps| deps.pop())
                .collect();

            let use_fixture_dependencies = normalized_use_fixtures
                .into_iter()
                .filter_map(|mut deps| deps.pop())
                .collect();

            return vec![NormalizedTestFunction::new(
                test_fn.name().function_name().to_string(),
                test_fn.name().clone(),
                location,
                HashMap::new(),
                fixture_dependencies,
                use_fixture_dependencies,
                missing_fixtures,
                test_fn.py_function().clone(),
                test_fn.tags().clone(),
            )];
        }

        // Create cartesian product
        let mut result = Vec::new();

        let dep_combinations = if normalized_deps.is_empty() {
            vec![vec![]]
        } else {
            cartesian_product(normalized_deps)
        };

        let use_fixture_combinations = if normalized_use_fixtures.is_empty() {
            vec![vec![]]
        } else {
            cartesian_product(normalized_use_fixtures)
        };

        for dep_combination in dep_combinations {
            for use_fixture_combination in &use_fixture_combinations {
                for test_param in &test_params {
                    // Build the parameter name string
                    let param_str = if test_param.is_empty() {
                        // Include fixture params in name
                        let fixture_params: Vec<String> = dep_combination
                            .iter()
                            .filter(|f| f.param().is_some())
                            .map(|f| {
                                format!(
                                    "{}={}",
                                    f.original_name()
                                        .as_ref()
                                        .map(|name| name.to_string())
                                        .unwrap_or(f.name().to_string()),
                                    stringify_param(py, f.param().unwrap())
                                )
                            })
                            .collect();
                        if fixture_params.is_empty() {
                            String::new()
                        } else {
                            fixture_params.join(",")
                        }
                    } else {
                        stringify_params(py, test_param)
                    };

                    let name = if param_str.is_empty() {
                        test_fn.name().function_name().to_string()
                    } else {
                        format!("{}[{}]", test_fn.name().function_name(), param_str)
                    };

                    let normalized = NormalizedTestFunction::new(
                        name,
                        test_fn.name().clone(),
                        location.clone(),
                        test_param.clone(),
                        dep_combination.clone(),
                        use_fixture_combination.clone(),
                        missing_fixtures.clone(),
                        test_fn.py_function().clone(),
                        test_fn.tags().clone(),
                    );

                    result.push(normalized);
                }
            }
        }

        result
    }

    /// Finds a fixture by name, searching in the current module and parent packages
    fn find_fixture<'a>(
        &self,
        name: &str,
        parents: &'a [&'a DiscoveredPackage],
        current: &'a DiscoveredModule,
    ) -> Option<&'a Fixture> {
        // First check the current module
        if let Some(fixture) = current.get_fixture(name) {
            return Some(fixture);
        }

        // Then check parent packages
        for (parent, _ancestors) in iter_with_ancestors(parents) {
            if let Some(fixture) = parent.get_fixture(name) {
                return Some(fixture);
            }
        }

        None
    }

    fn normalize_module(
        &mut self,
        py: Python<'_>,
        module: &DiscoveredModule,
        parents: &[&DiscoveredPackage],
        fixture_manager: &mut FixtureManager,
    ) -> NormalizedModule {
        tracing::debug!("Normalizing module: {}", module.path());

        let mut normalized_test_functions = Vec::new();
        let mut normalized_fixtures = Vec::new();

        // Normalize all fixtures in the module
        for fixture in module.fixtures() {
            let normalized = self.normalize_fixture(py, fixture, parents, module);
            normalized_fixtures.extend(normalized);
        }

        // Normalize all test functions
        for test_function in module.test_functions() {
            let normalized_tests = self.normalize_test_function(py, test_function, parents, module);

            for normalized_test in normalized_tests {
                tracing::debug!(
                    "Normalized test: {}",
                    normalized_test.synthetic_name.clone()
                );
                normalized_test_functions.push(normalized_test);
            }
        }

        NormalizedModule {
            path: module.module_path().clone(),
            test_functions: normalized_test_functions,
            fixtures: normalized_fixtures,
            type_: module.module_type(),
            source_text: module.source_text().clone(),
            line_index: module.line_index().clone(),
        }
    }

    fn normalize_package_impl(
        &mut self,
        py: Python<'_>,
        package: &DiscoveredPackage,
        parents: &[&DiscoveredPackage],
        fixture_manager: &mut FixtureManager,
    ) -> NormalizedPackage {
        let mut new_parents = parents.to_vec();
        new_parents.push(package);

        let mut modules = HashMap::new();
        for (path, module) in package.modules() {
            let normalized_module =
                self.normalize_module(py, module, &new_parents, fixture_manager);
            modules.insert(path.clone(), normalized_module);
        }

        let mut packages = HashMap::new();
        for (path, sub_package) in package.packages() {
            let normalized_package =
                self.normalize_package_impl(py, sub_package, &new_parents, fixture_manager);
            packages.insert(path.clone(), normalized_package);
        }

        NormalizedPackage {
            path: package.path().clone(),
            modules,
            packages,
            configuration_module_path: package
                .configuration_module()
                .map(|m| m.module_path().clone()),
        }
    }
}
