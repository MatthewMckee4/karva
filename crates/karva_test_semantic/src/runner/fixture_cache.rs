//! Scope-aware cache for initialized Python fixture values.

use std::collections::{HashMap, HashSet};

use pyo3::prelude::*;

use crate::runner::scoped_storage::{ScopeKey, ScopedStorage};

/// Selected parameters that distinguish values of a parametrized fixture.
#[derive(Debug)]
pub(super) struct FixtureCacheKey {
    pub(super) parameters: Vec<(String, Py<PyAny>)>,
}

impl FixtureCacheKey {
    fn matches(&self, py: Python<'_>, other: &Self) -> PyResult<bool> {
        if self.parameters.len() != other.parameters.len() {
            return Ok(false);
        }
        for ((name, value), (other_name, other_value)) in
            self.parameters.iter().zip(&other.parameters)
        {
            if name != other_name {
                return Ok(false);
            }
            let equal = match value.bind(py).eq(other_value.bind(py)) {
                Ok(equal) => equal,
                Err(error)
                    if error.is_instance_of::<pyo3::exceptions::PyValueError>(py)
                        || error.is_instance_of::<pyo3::exceptions::PyRuntimeError>(py) =>
                {
                    value.bind(py).is(other_value.bind(py))
                }
                Err(error) => return Err(error),
            };
            if !equal {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

#[derive(Debug)]
struct CachedFixture {
    key: Option<FixtureCacheKey>,
    value: Py<PyAny>,
}

/// Result of looking up the sole active instance of one fixture definition.
pub(super) enum FixtureCacheLookup {
    Hit(Py<PyAny>),
    Vacant,
    ParameterChanged,
}

/// Caches fixture values at different scope levels.
///
/// Fixtures are cached based on their declared scope to avoid redundant
/// setup when the same fixture is used multiple times within a scope.
#[derive(Debug, Default)]
pub(super) struct FixtureCache {
    /// Fixture values isolated by their declared lifetime scope.
    storage: ScopedStorage<HashMap<String, CachedFixture>>,

    /// Active dependents that must finish before a parameterized dependency.
    dependents: Option<HashMap<String, HashSet<String>>>,
}

impl FixtureCache {
    /// Returns a cached value or reports that its active parameter changed.
    pub(super) fn lookup(
        &self,
        py: Python<'_>,
        fixture: &str,
        key: Option<&FixtureCacheKey>,
        scope: ScopeKey<'_>,
    ) -> PyResult<FixtureCacheLookup> {
        self.storage
            .with(scope, |values| {
                let Some(cached) = values.get(fixture) else {
                    return Ok(FixtureCacheLookup::Vacant);
                };
                let matches = match (&cached.key, key) {
                    (None, None) => true,
                    (Some(cached), Some(requested)) => cached.matches(py, requested)?,
                    _ => false,
                };
                if matches {
                    Ok(FixtureCacheLookup::Hit(cached.value.clone_ref(py)))
                } else {
                    Ok(FixtureCacheLookup::ParameterChanged)
                }
            })
            .unwrap_or(Ok(FixtureCacheLookup::Vacant))
    }

    /// Caches a fixture value until its declared scope completes.
    pub(super) fn insert(
        &mut self,
        fixture: String,
        key: Option<FixtureCacheKey>,
        value: Py<PyAny>,
        scope: ScopeKey<'_>,
        dependencies: impl IntoIterator<Item = String>,
    ) {
        for dependency in dependencies {
            self.dependents
                .get_or_insert_with(HashMap::new)
                .entry(dependency)
                .or_default()
                .insert(fixture.clone());
        }
        self.storage.with_mut(scope, |values| {
            values.insert(fixture, CachedFixture { key, value });
        });
    }

    /// Drops the active instance of one fixture after parameter-driven teardown.
    pub(super) fn remove(&mut self, fixture: &str) {
        self.storage.for_each_mut(|values| {
            drop(values.remove(fixture));
        });
        self.remove_dependency_edges(fixture);
    }

    /// Records a dependency discovered through `request.getfixturevalue`.
    pub(super) fn add_dependency(&mut self, dependency: String, dependent: String) {
        self.dependents
            .get_or_insert_with(HashMap::new)
            .entry(dependency)
            .or_default()
            .insert(dependent);
    }

    /// Returns whether an active fixture cache key includes one parameter.
    pub(super) fn uses_parameter(
        &self,
        fixture: &str,
        parameter: &str,
        scope: ScopeKey<'_>,
    ) -> bool {
        self.storage
            .with(scope, |values| {
                values
                    .get(fixture)
                    .and_then(|cached| cached.key.as_ref())
                    .is_some_and(|key| key.parameters.iter().any(|(name, _)| name == parameter))
            })
            .is_some_and(|uses_parameter| uses_parameter)
    }

    /// Returns active fixture definitions that depend on `fixture`.
    pub(super) fn dependents(&self, fixture: &str) -> Vec<String> {
        self.dependents
            .as_ref()
            .and_then(|dependents| dependents.get(fixture))
            .map_or_else(Vec::new, |dependents| dependents.iter().cloned().collect())
    }

    /// Drops every cached value owned by one completed scope.
    pub(super) fn clear_scope(&mut self, scope: ScopeKey<'_>) {
        let fixtures = self.storage.take(scope);
        for fixture in fixtures.keys() {
            self.remove_dependency_edges(fixture);
        }
    }

    fn remove_dependency_edges(&mut self, fixture: &str) {
        let Some(dependency_graph) = &mut self.dependents else {
            return;
        };
        dependency_graph.remove(fixture);
        dependency_graph.retain(|_, dependents| {
            dependents.remove(fixture);
            !dependents.is_empty()
        });
        if dependency_graph.is_empty() {
            self.dependents = None;
        }
    }
}
