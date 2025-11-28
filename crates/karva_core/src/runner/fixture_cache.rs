use std::{collections::HashMap, sync::Mutex};

use pyo3::prelude::*;

use crate::extensions::fixtures::FixtureScope;

/// Manages caching of fixture values based on their scope.
#[derive(Debug, Default)]
pub struct FixtureCache {
    session: Mutex<HashMap<String, Py<PyAny>>>,

    package: Mutex<HashMap<String, Py<PyAny>>>,

    module: Mutex<HashMap<String, Py<PyAny>>>,

    function: Mutex<HashMap<String, Py<PyAny>>>,
}

impl FixtureCache {
    /// Get a fixture value from the cache based on its scope
    pub fn get(&self, name: &str, scope: FixtureScope) -> Option<Py<PyAny>> {
        match scope {
            FixtureScope::Session => self.session.lock().unwrap().get(name).cloned(),
            FixtureScope::Package => self.package.lock().unwrap().get(name).cloned(),
            FixtureScope::Module => self.module.lock().unwrap().get(name).cloned(),
            FixtureScope::Function => self.function.lock().unwrap().get(name).cloned(),
        }
    }

    /// Insert a fixture value into the cache based on its scope
    pub fn insert(&self, name: String, value: Py<PyAny>, scope: FixtureScope) {
        match scope {
            FixtureScope::Session => {
                self.session.lock().unwrap().insert(name, value);
            }
            FixtureScope::Package => {
                self.package.lock().unwrap().insert(name, value);
            }
            FixtureScope::Module => {
                self.module.lock().unwrap().insert(name, value);
            }
            FixtureScope::Function => {
                self.function.lock().unwrap().insert(name, value);
            }
        }
    }

    pub(crate) fn clear_fixtures(&self, scope: FixtureScope) {
        match scope {
            FixtureScope::Function => self.function.lock().unwrap().clear(),
            FixtureScope::Module => self.module.lock().unwrap().clear(),
            FixtureScope::Package => self.package.lock().unwrap().clear(),
            FixtureScope::Session => self.session.lock().unwrap().clear(),
        }
    }
}
