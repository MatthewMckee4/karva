//! Scope-aware cache for initialized Python fixture values.

use std::collections::HashMap;

use pyo3::prelude::*;

use crate::extensions::fixtures::FixtureScope;
use crate::runner::scoped_storage::ScopedStorage;

/// Caches fixture values at different scope levels.
///
/// Fixtures are cached based on their declared scope to avoid redundant
/// setup when the same fixture is used multiple times within a scope.
#[derive(Debug, Default)]
pub(super) struct FixtureCache {
    /// Fixture values isolated by their declared lifetime scope.
    storage: ScopedStorage<HashMap<String, Py<PyAny>>>,
}

impl FixtureCache {
    /// Returns a new Python reference to a cached fixture value.
    pub(super) fn get(&self, py: Python<'_>, name: &str, scope: FixtureScope) -> Option<Py<PyAny>> {
        self.storage
            .get(scope)
            .borrow()
            .get(name)
            .map(|value| value.clone_ref(py))
    }

    /// Caches a fixture value until its declared scope completes.
    pub(super) fn insert(&self, name: String, value: Py<PyAny>, scope: FixtureScope) {
        self.storage.get(scope).borrow_mut().insert(name, value);
    }

    /// Drops every cached value owned by one completed scope.
    pub(super) fn clear_scope(&self, scope: FixtureScope) {
        self.storage.get(scope).borrow_mut().clear();
    }
}
