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
    /// Get a fixture value from the cache based on its scope
    pub fn get(&self, py: Python, name: &str, scope: FixtureScope) -> Option<Py<PyAny>> {
        match scope {
            FixtureScope::Session => self.session.borrow().get(name).map(|v| v.clone_ref(py)),
            FixtureScope::Package => self.package.borrow().get(name).map(|v| v.clone_ref(py)),
            FixtureScope::Module => self.module.borrow().get(name).map(|v| v.clone_ref(py)),
            FixtureScope::Function => self.function.borrow().get(name).map(|v| v.clone_ref(py)),
        }
    }

    /// Insert a fixture value into the cache based on its scope
    pub fn insert(&self, name: String, value: Py<PyAny>, scope: FixtureScope) {
        match scope {
            FixtureScope::Session => {
                self.session.borrow_mut().insert(name, value);
            }
            FixtureScope::Package => {
                self.package.borrow_mut().insert(name, value);
            }
            FixtureScope::Module => {
                self.module.borrow_mut().insert(name, value);
            }
            FixtureScope::Function => {
                self.function.borrow_mut().insert(name, value);
            }
        }
    }

    pub(crate) fn clear_fixtures(&self, scope: FixtureScope) {
        match scope {
            FixtureScope::Function => self.function.borrow_mut().clear(),
            FixtureScope::Module => self.module.borrow_mut().clear(),
            FixtureScope::Package => self.package.borrow_mut().clear(),
            FixtureScope::Session => self.session.borrow_mut().clear(),
        }
    }
}
