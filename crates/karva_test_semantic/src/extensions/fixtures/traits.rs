use std::fmt::Debug;

use pyo3::Python;
use ruff_python_ast::StmtFunctionDef;

use crate::discovery::{DiscoveredModule, DiscoveredPackage};
use crate::extensions::fixtures::{DiscoveredFixture, FixtureScope};

/// This trait is used to get all fixtures (from a module or package) that have a given scope.
///
/// For example, if we are in a test module, we want to get all fixtures used in the test module.
/// If we are in a package, we want to get all fixtures used in the package from the configuration module.
pub trait HasFixtures<'a>: Debug {
    /// Get a fixture with the given name
    fn get_fixture(&'a self, fixture_name: &str) -> Option<&'a DiscoveredFixture>;

    /// Get all autouse fixtures
    ///
    /// If this returns a non-empty list, it means that the module or package has a configuration module.
    fn auto_use_fixtures(&'a self, scopes: &[FixtureScope]) -> Vec<&'a DiscoveredFixture>;
}

impl<'a> HasFixtures<'a> for DiscoveredModule {
    fn get_fixture(&'a self, fixture_name: &str) -> Option<&'a DiscoveredFixture> {
        self.fixtures()
            .iter()
            .find(|f| f.name().function_name() == fixture_name)
    }

    fn auto_use_fixtures(&'a self, scopes: &[FixtureScope]) -> Vec<&'a DiscoveredFixture> {
        self.fixtures()
            .iter()
            .filter(|f| f.auto_use() && scopes.contains(&f.scope()))
            .collect()
    }
}

impl<'a> HasFixtures<'a> for DiscoveredPackage {
    fn get_fixture(&'a self, fixture_name: &str) -> Option<&'a DiscoveredFixture> {
        self.configuration_module_impl()
            .and_then(|module| module.get_fixture(fixture_name))
    }

    fn auto_use_fixtures(&'a self, scopes: &[FixtureScope]) -> Vec<&'a DiscoveredFixture> {
        let mut fixtures = Vec::new();

        if let Some(module) = self.configuration_module_impl() {
            fixtures.extend(module.auto_use_fixtures(scopes));
        }

        fixtures
    }
}

impl<'a> HasFixtures<'a> for &'a DiscoveredPackage {
    fn get_fixture(&'a self, fixture_name: &str) -> Option<&'a DiscoveredFixture> {
        (*self).get_fixture(fixture_name)
    }

    fn auto_use_fixtures(&'a self, scopes: &[FixtureScope]) -> Vec<&'a DiscoveredFixture> {
        (*self).auto_use_fixtures(scopes)
    }
}

/// This trait is used to represent an object that may require fixtures to be called before it is run.
pub trait RequiresFixtures {
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

impl RequiresFixtures for DiscoveredFixture {
    fn required_fixtures(&self, py: Python<'_>) -> Vec<String> {
        self.stmt_function_def.required_fixtures(py)
    }
}
