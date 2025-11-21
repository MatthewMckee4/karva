use std::collections::HashMap;

use pyo3::prelude::*;

use crate::extensions::fixtures::FixtureScope;

/// Manages caching of fixture values based on their scope.
///
/// Fixtures are cached at different levels:
/// - Session: Cached for the entire test session
/// - Package: Cached for a package (cleared after package completes)
/// - Module: Cached for a module (cleared after module completes)
/// - Function: Cached per test function (cleared after each test)
pub struct FixtureCache {
    /// Session-scoped fixtures - persist for the entire test run
    session: HashMap<String, Py<PyAny>>,

    /// Package-scoped fixtures - cleared after each package
    package: HashMap<String, Py<PyAny>>,

    /// Module-scoped fixtures - cleared after each module
    module: HashMap<String, Py<PyAny>>,

    /// Function-scoped fixtures - cleared after each test function
    function: HashMap<String, Py<PyAny>>,
}

impl FixtureCache {
    pub fn new() -> Self {
        Self {
            session: HashMap::new(),
            package: HashMap::new(),
            module: HashMap::new(),
            function: HashMap::new(),
        }
    }

    /// Get a fixture value from the cache based on its scope
    pub fn get(&self, name: &str, scope: FixtureScope) -> Option<&Py<PyAny>> {
        match scope {
            FixtureScope::Session => self.session.get(name),
            FixtureScope::Package => self.package.get(name),
            FixtureScope::Module => self.module.get(name),
            FixtureScope::Function => self.function.get(name),
        }
    }

    /// Insert a fixture value into the cache based on its scope
    pub fn insert(&mut self, name: String, value: Py<PyAny>, scope: FixtureScope) {
        match scope {
            FixtureScope::Session => {
                self.session.insert(name, value);
            }
            FixtureScope::Package => {
                self.package.insert(name, value);
            }
            FixtureScope::Module => {
                self.module.insert(name, value);
            }
            FixtureScope::Function => {
                self.function.insert(name, value);
            }
        }
    }

    /// Clear fixtures at a specific scope level
    pub fn clear_scope(&mut self, scope: FixtureScope) {
        match scope {
            FixtureScope::Session => self.session.clear(),
            FixtureScope::Package => self.package.clear(),
            FixtureScope::Module => self.module.clear(),
            FixtureScope::Function => self.function.clear(),
        }
    }

    /// Clear function-scoped fixtures (called after each test)
    pub fn clear_function_fixtures(&mut self) {
        self.function.clear();
    }

    /// Clear module-scoped fixtures (called after each module)
    pub fn clear_module_fixtures(&mut self) {
        self.module.clear();
    }

    /// Clear package-scoped fixtures (called after each package)
    pub fn clear_package_fixtures(&mut self) {
        self.package.clear();
    }

    /// Clear session-scoped fixtures (called at the end of the session)
    pub fn clear_session_fixtures(&mut self) {
        self.session.clear();
    }
}

impl Default for FixtureCache {
    fn default() -> Self {
        Self::new()
    }
}
