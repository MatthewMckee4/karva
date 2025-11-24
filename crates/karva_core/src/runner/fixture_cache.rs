use std::collections::HashMap;

use pyo3::prelude::*;

use crate::extensions::fixtures::FixtureScope;

/// Manages caching of fixture values based on their scope.
#[derive(Debug, Default)]
pub struct FixtureCache {
    session: HashMap<String, Py<PyAny>>,

    package: HashMap<String, Py<PyAny>>,

    module: HashMap<String, Py<PyAny>>,

    function: HashMap<String, Py<PyAny>>,
}

impl FixtureCache {
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

    pub(crate) fn clear_fixtures(&mut self, scope: FixtureScope) {
        match scope {
            FixtureScope::Function => self.function.clear(),
            FixtureScope::Module => self.module.clear(),
            FixtureScope::Package => self.package.clear(),
            FixtureScope::Session => self.session.clear(),
        }
    }
}
