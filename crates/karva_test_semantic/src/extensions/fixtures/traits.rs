use std::fmt::Debug;

use pyo3::Python;
use ruff_python_ast::StmtFunctionDef;

use crate::discovery::{DiscoveredModule, DiscoveredPackage};
use crate::extensions::fixtures::{DiscoveredFixture, FixtureScope, RejectedFixture};

/// This trait is used to get all fixtures (from a module or package) that have a given scope.
///
/// For example, if we are in a test module, we want to get all fixtures used in the test module.
/// If we are in a package, we want to get all fixtures used in the package from the configuration module.
pub trait HasFixtures<'a>: Debug {
    /// Get a fixture with the given name
    fn get_fixture(&'a self, fixture_name: &str) -> Option<&'a DiscoveredFixture>;

    /// Get a fixture definition rejected during discovery.
    fn get_rejected_fixture(&'a self, fixture_name: &str) -> Option<&'a RejectedFixture>;

    /// Get all autouse fixtures
    ///
    /// If this returns a non-empty list, it means that the module or package has a configuration module.
    fn auto_use_fixtures(&'a self, scopes: &[FixtureScope]) -> Vec<&'a DiscoveredFixture>;

    /// Names visible directly from this provider, in lookup precedence order.
    fn fixture_names(&'a self) -> Vec<&'a str>;
}

impl<'a> HasFixtures<'a> for DiscoveredModule {
    fn get_fixture(&'a self, fixture_name: &str) -> Option<&'a DiscoveredFixture> {
        self.fixtures()
            .iter()
            .find(|f| f.name().function_name() == fixture_name)
    }

    fn get_rejected_fixture(&'a self, fixture_name: &str) -> Option<&'a RejectedFixture> {
        self.rejected_fixture(fixture_name)
    }

    fn auto_use_fixtures(&'a self, scopes: &[FixtureScope]) -> Vec<&'a DiscoveredFixture> {
        self.fixtures()
            .iter()
            .filter(|f| f.auto_use() && scopes.contains(&f.scope()))
            .collect()
    }

    fn fixture_names(&'a self) -> Vec<&'a str> {
        self.fixtures()
            .iter()
            .map(|fixture| fixture.name().function_name())
            .collect()
    }
}

impl<'a> HasFixtures<'a> for DiscoveredPackage {
    fn get_fixture(&'a self, fixture_name: &str) -> Option<&'a DiscoveredFixture> {
        self.configuration_module_impl()
            .and_then(|module| module.get_fixture(fixture_name))
            .or_else(|| {
                self.framework_module_impl()
                    .and_then(|module| module.get_fixture(fixture_name))
            })
    }

    fn get_rejected_fixture(&'a self, fixture_name: &str) -> Option<&'a RejectedFixture> {
        self.configuration_module_impl()
            .and_then(|module| module.get_rejected_fixture(fixture_name))
            .or_else(|| {
                self.framework_module_impl()
                    .and_then(|module| module.get_rejected_fixture(fixture_name))
            })
    }

    fn auto_use_fixtures(&'a self, scopes: &[FixtureScope]) -> Vec<&'a DiscoveredFixture> {
        let mut fixtures: Vec<&'a DiscoveredFixture> = Vec::new();
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();

        // User-defined conftest fixtures win on name collision, so they are
        // collected first and framework fixtures with the same unqualified
        // name are dropped.
        if let Some(module) = self.configuration_module_impl() {
            for fixture in module.auto_use_fixtures(scopes) {
                if seen.insert(fixture.name().function_name()) {
                    fixtures.push(fixture);
                }
            }
        }

        if let Some(module) = self.framework_module_impl() {
            for fixture in module.auto_use_fixtures(scopes) {
                if seen.insert(fixture.name().function_name()) {
                    fixtures.push(fixture);
                }
            }
        }

        fixtures
    }

    fn fixture_names(&'a self) -> Vec<&'a str> {
        let mut names = Vec::new();
        let mut seen = std::collections::HashSet::new();
        if let Some(module) = self.configuration_module_impl() {
            for name in module.fixture_names() {
                if seen.insert(name) {
                    names.push(name);
                }
            }
        }
        if let Some(module) = self.framework_module_impl() {
            for name in module.fixture_names() {
                if seen.insert(name) {
                    names.push(name);
                }
            }
        }
        names
    }
}

impl<'a> HasFixtures<'a> for &'a DiscoveredPackage {
    fn get_fixture(&'a self, fixture_name: &str) -> Option<&'a DiscoveredFixture> {
        (*self).get_fixture(fixture_name)
    }

    fn get_rejected_fixture(&'a self, fixture_name: &str) -> Option<&'a RejectedFixture> {
        (*self).get_rejected_fixture(fixture_name)
    }

    fn auto_use_fixtures(&'a self, scopes: &[FixtureScope]) -> Vec<&'a DiscoveredFixture> {
        (*self).auto_use_fixtures(scopes)
    }

    fn fixture_names(&'a self) -> Vec<&'a str> {
        (*self).fixture_names()
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
        self.stmt_function_def().required_fixtures(py)
    }
}
