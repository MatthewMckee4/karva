use std::fmt::Debug;

use pyo3::Python;
use ruff_python_ast::StmtFunctionDef;

use crate::{
    discovery::{DiscoveredModule, DiscoveredPackage},
    extensions::fixtures::{Fixture, FixtureScope},
};

/// This trait is used to get all fixtures (from a module or package) that have a given scope.
///
/// For example, if we are in a test module, we want to get all fixtures used in the test module.
/// If we are in a package, we want to get all fixtures used in the package from the configuration module.
pub trait HasFixtures<'a>: Debug {
    /// Get all fixtures with the given names and scopes
    ///
    /// If fixture names is empty, return all fixtures.
    fn fixtures(
        &'a self,
        scopes: &[FixtureScope],
        fixture_names: Option<&[String]>,
    ) -> Vec<&'a Fixture> {
        let mut fixtures = Vec::new();
        for fixture in self.all_fixtures(fixture_names) {
            if scopes.contains(&fixture.scope()) {
                fixtures.push(fixture);
            }
        }
        fixtures
    }

    /// Get a fixture with the given name
    fn get_fixture(&'a self, fixture_name: &str) -> Option<&'a Fixture>;

    /// Get all fixtures with the given names
    ///
    /// If fixture names is empty, return all fixtures.
    fn all_fixtures(&'a self, fixture_names: Option<&[String]>) -> Vec<&'a Fixture>;

    /// The module where the fixtures are being found in.
    fn fixture_module(&'a self) -> Option<&'a DiscoveredModule>;
}

impl<'a> HasFixtures<'a> for DiscoveredModule {
    fn all_fixtures(&'a self, fixture_names: Option<&[String]>) -> Vec<&'a Fixture> {
        let Some(fixture_names) = fixture_names else {
            return self.fixtures().iter().collect();
        };

        self.fixtures()
            .iter()
            .filter(|f| {
                if f.auto_use() {
                    true
                } else {
                    fixture_names.contains(&f.name().function_name().to_string())
                }
            })
            .collect()
    }

    fn fixture_module(&'a self) -> Option<&'a DiscoveredModule> {
        Some(self)
    }

    fn get_fixture(&'a self, fixture_name: &str) -> Option<&'a Fixture> {
        self.fixtures()
            .iter()
            .find(|f| f.name().function_name() == fixture_name)
    }
}

impl<'a> HasFixtures<'a> for DiscoveredPackage {
    fn all_fixtures(&'a self, fixture_names: Option<&[String]>) -> Vec<&'a Fixture> {
        let mut fixtures = Vec::new();

        if let Some(module) = self.configuration_module() {
            fixtures.extend(module.all_fixtures(fixture_names));
        }

        fixtures
    }

    fn fixture_module(&'a self) -> Option<&'a DiscoveredModule> {
        self.configuration_module()
    }

    fn get_fixture(&'a self, fixture_name: &str) -> Option<&'a Fixture> {
        self.configuration_module()
            .and_then(|module| module.get_fixture(fixture_name))
    }
}

impl<'a> HasFixtures<'a> for &'a DiscoveredPackage {
    fn all_fixtures(&'a self, fixture_names: Option<&[String]>) -> Vec<&'a Fixture> {
        (*self).all_fixtures(fixture_names)
    }

    fn fixture_module(&'a self) -> Option<&'a DiscoveredModule> {
        (*self).fixture_module()
    }

    fn get_fixture(&'a self, fixture_name: &str) -> Option<&'a Fixture> {
        (*self).get_fixture(fixture_name)
    }
}

/// This trait is used to represent an object that may require fixtures to be called before it is run.
pub trait RequiresFixtures {
    #[cfg(test)]
    fn uses_fixture(&self, py: Python<'_>, fixture_name: &str) -> bool {
        self.required_fixtures(py)
            .contains(&fixture_name.to_string())
    }

    fn required_fixtures(&self, py: Python<'_>) -> Vec<String>;
}

impl RequiresFixtures for StmtFunctionDef {
    fn required_fixtures(&self, _py: Python<'_>) -> Vec<String> {
        let mut required_fixtures = Vec::new();

        for parameter in self.parameters.iter_non_variadic_params() {
            required_fixtures.push(parameter.parameter.name.as_str().to_string());
        }

        required_fixtures
    }
}

impl RequiresFixtures for Fixture {
    fn required_fixtures(&self, py: Python<'_>) -> Vec<String> {
        self.function_definition.required_fixtures(py)
    }
}

impl RequiresFixtures for DiscoveredPackage {
    fn required_fixtures(&self, py: Python<'_>) -> Vec<String> {
        let mut fixtures = Vec::new();

        for module in self.modules().values() {
            fixtures.extend(module.required_fixtures(py));
        }

        for sub_package in self.packages().values() {
            fixtures.extend(sub_package.required_fixtures(py));
        }

        fixtures
    }
}

impl RequiresFixtures for DiscoveredModule {
    fn required_fixtures(&self, py: Python<'_>) -> Vec<String> {
        let mut fixtures = Vec::new();

        for test_function in &self.test_functions() {
            fixtures.extend(test_function.required_fixtures(py));
        }

        for fixture in self.fixtures() {
            fixtures.extend(fixture.required_fixtures(py));
        }

        fixtures
    }
}
