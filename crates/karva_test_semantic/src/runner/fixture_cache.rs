//! Scope-aware cache for initialized Python fixture values.

use std::collections::HashMap;

use pyo3::prelude::*;

use crate::runner::scoped_storage::{ScopeKey, ScopedStorage};

/// Selected parameters that distinguish values of a parametrized fixture.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(super) struct FixtureCacheKey {
    pub(super) fixture: String,
    pub(super) parameters: Vec<(String, usize)>,
}

#[derive(Debug, Default)]
struct FixtureValues {
    unparameterized: HashMap<String, Py<PyAny>>,
    parameterized: HashMap<FixtureCacheKey, Py<PyAny>>,
}

/// Caches fixture values at different scope levels.
///
/// Fixtures are cached based on their declared scope to avoid redundant
/// setup when the same fixture is used multiple times within a scope.
#[derive(Debug, Default)]
pub(super) struct FixtureCache {
    /// Fixture values isolated by their declared lifetime scope.
    storage: ScopedStorage<FixtureValues>,
}

impl FixtureCache {
    /// Returns a new Python reference to a cached fixture value.
    pub(super) fn get(
        &self,
        py: Python<'_>,
        fixture: &str,
        key: Option<&FixtureCacheKey>,
        scope: ScopeKey<'_>,
    ) -> Option<Py<PyAny>> {
        self.storage
            .with(scope, |values| {
                key.map_or_else(
                    || values.unparameterized.get(fixture),
                    |key| values.parameterized.get(key),
                )
                .map(|value| value.clone_ref(py))
            })
            .flatten()
    }

    /// Caches a fixture value until its declared scope completes.
    pub(super) fn insert(
        &mut self,
        fixture: String,
        key: Option<FixtureCacheKey>,
        value: Py<PyAny>,
        scope: ScopeKey<'_>,
    ) {
        self.storage.with_mut(scope, |values| {
            if let Some(key) = key {
                values.parameterized.insert(key, value);
            } else {
                values.unparameterized.insert(fixture, value);
            }
        });
    }

    /// Drops every cached value owned by one completed scope.
    pub(super) fn clear_scope(&mut self, scope: ScopeKey<'_>) {
        drop(self.storage.take(scope));
    }
}
