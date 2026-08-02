//! Scope-aware cache for initialized Python fixture values.

use std::collections::HashMap;

use pyo3::prelude::*;

use crate::runner::scoped_storage::{ScopeKey, ScopedStorage};

/// Fixture identity plus selected parameters that affect its value.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct FixtureCacheKey {
    pub(super) fixture: String,
    pub(super) parameters: Vec<(String, usize)>,
}

/// Caches fixture values at different scope levels.
///
/// Fixtures are cached based on their declared scope to avoid redundant
/// setup when the same fixture is used multiple times within a scope.
#[derive(Debug, Default)]
pub(super) struct FixtureCache {
    /// Fixture values isolated by their declared lifetime scope.
    storage: ScopedStorage<HashMap<FixtureCacheKey, Py<PyAny>>>,
}

impl FixtureCache {
    /// Returns a new Python reference to a cached fixture value.
    pub(super) fn get(
        &self,
        py: Python<'_>,
        key: &FixtureCacheKey,
        scope: ScopeKey<'_>,
    ) -> Option<Py<PyAny>> {
        self.storage
            .with(scope, |values| {
                values.get(key).map(|value| value.clone_ref(py))
            })
            .flatten()
    }

    /// Caches a fixture value until its declared scope completes.
    pub(super) fn insert(&mut self, key: FixtureCacheKey, value: Py<PyAny>, scope: ScopeKey<'_>) {
        self.storage.with_mut(scope, |values| {
            values.insert(key, value);
        });
    }

    /// Drops every cached value owned by one completed scope.
    pub(super) fn clear_scope(&mut self, scope: ScopeKey<'_>) {
        drop(self.storage.take(scope));
    }
}
