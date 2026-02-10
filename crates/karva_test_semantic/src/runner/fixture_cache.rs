use std::cell::RefCell;
use std::collections::HashMap;

use pyo3::prelude::*;

use crate::extensions::fixtures::FixtureScope;

/// Caches fixture values at different scope levels.
///
/// Fixtures are cached based on their declared scope to avoid redundant
/// setup when the same fixture is used multiple times within a scope.
#[derive(Debug, Default)]
pub struct FixtureCache {
    /// Session-scoped fixture values (persist for entire test run).
    session: RefCell<HashMap<String, Py<PyAny>>>,

    /// Package-scoped fixture values (cleared after each package).
    package: RefCell<HashMap<String, Py<PyAny>>>,

    /// Module-scoped fixture values (cleared after each module).
    module: RefCell<HashMap<String, Py<PyAny>>>,

    /// Function-scoped fixture values (cleared after each test).
    function: RefCell<HashMap<String, Py<PyAny>>>,
}

impl FixtureCache {
    fn scope_storage(&self, scope: FixtureScope) -> &RefCell<HashMap<String, Py<PyAny>>> {
        match scope {
            FixtureScope::Session => &self.session,
            FixtureScope::Package => &self.package,
            FixtureScope::Module => &self.module,
            FixtureScope::Function => &self.function,
        }
    }

    /// Get a fixture value from the cache based on its scope
    pub(crate) fn get(&self, py: Python, name: &str, scope: FixtureScope) -> Option<Py<PyAny>> {
        self.scope_storage(scope)
            .borrow()
            .get(name)
            .map(|v| v.clone_ref(py))
    }

    /// Insert a fixture value into the cache based on its scope
    pub(crate) fn insert(&self, name: String, value: Py<PyAny>, scope: FixtureScope) {
        self.scope_storage(scope).borrow_mut().insert(name, value);
    }

    pub(crate) fn clear_fixtures(&self, scope: FixtureScope) {
        self.scope_storage(scope).borrow_mut().clear();
    }
}
