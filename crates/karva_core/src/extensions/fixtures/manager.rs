use indexmap::IndexMap;
use pyo3::prelude::*;

use crate::{
    diagnostic::Diagnostic,
    discovery::DiscoveredPackage,
    extensions::fixtures::{
        Finalizer, Finalizers, Fixture, FixtureGetResult, FixtureScope, HasFixtures,
        RequiresFixtures, builtins::get_builtin_fixture,
    },
    name::QualifiedFunctionName,
    utils::iter_with_ancestors,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FixtureKey {
    pub(crate) name: QualifiedFunctionName,
    pub(crate) scope: FixtureScope,
}

/// Collection of fixtures and their finalizers for a specific scope.
#[derive(Debug, Default, Clone)]
pub(crate) struct FixtureCollection {
    /// Map of fixture names to their resolved Python values
    fixtures: IndexMap<FixtureKey, FixtureGetResult>,
    /// List of cleanup functions to execute when this collection is reset
    finalizers: Vec<Finalizer>,
}

impl FixtureCollection {
    fn insert_fixture(&mut self, fixture_name: FixtureKey, fixture_return: FixtureGetResult) {
        self.fixtures.insert(fixture_name, fixture_return);
    }

    fn insert_finalizer(&mut self, finalizer: Finalizer) {
        self.finalizers.push(finalizer);
    }

    /// Get a fixture by name
    ///
    /// If the fixture name matches a built-in fixture,
    /// it creates the fixture on-demand and stores it.
    fn get_fixture(&self, py: Python<'_>, fixture_name: &str) -> Option<FixtureGetResult> {
        if let Some((_, fixture)) = self
            .fixtures
            .iter()
            .rev()
            .find(|(key, _)| key.name.function_name() == fixture_name)
        {
            return Some(fixture.clone());
        }

        get_builtin_fixture(py, fixture_name)
    }

    fn clear_finalizers(&mut self, scope: FixtureScope) -> Finalizers {
        let mut finalizers_with_scope = Vec::new();
        let mut remaining_finalizers = Vec::new();

        for finalizer in self.finalizers.drain(..) {
            if finalizer.scope == scope {
                finalizers_with_scope.push(finalizer);
            } else {
                remaining_finalizers.push(finalizer);
            }
        }

        self.finalizers = remaining_finalizers;

        Finalizers::new(finalizers_with_scope)
    }

    fn clear_fixtures(&mut self, scope: FixtureScope) {
        self.fixtures.retain(|key, _| key.scope != scope);
    }
}

/// Manages fixtures for a specific scope in the test execution hierarchy.
///
/// The `FixtureManager` follows a hierarchical structure where each manager
/// can have a parent, allowing fixture resolution to traverse up the scope
/// chain (function -> module -> package -> session). This enables proper
/// fixture inheritance and dependency resolution across different test scopes.
#[derive(Debug)]
pub(crate) struct FixtureManager {
    /// Reference to the parent manager in the scope hierarchy
    collection: FixtureCollection,

    /// The diagnostics from creating fixtures
    diagnostics: Vec<Diagnostic>,
}

impl FixtureManager {
    pub(crate) fn new() -> Self {
        Self {
            collection: FixtureCollection::default(),
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn clear_diagnostics(&mut self) -> Vec<Diagnostic> {
        self.diagnostics.drain(..).collect()
    }

    pub(crate) fn insert_fixture(&mut self, fixture_return: FixtureGetResult, fixture: &Fixture) {
        self.collection.insert_fixture(
            FixtureKey {
                name: fixture.name().clone(),
                scope: fixture.scope(),
            },
            fixture_return,
        );
    }

    pub(crate) fn insert_finalizer(&mut self, finalizer: Finalizer) {
        self.collection.insert_finalizer(finalizer);
    }

    /// Recursively resolves and executes fixture dependencies.
    ///
    /// This method ensures that all dependencies of a fixture are resolved and executed
    /// before the fixture itself is called. It performs a depth-first traversal of the
    /// dependency graph, checking both the current scope and parent scopes for required fixtures.
    fn ensure_fixture_dependencies<'proj>(
        &mut self,
        py: Python<'_>,
        parents: &[&'proj DiscoveredPackage],
        current: &'proj dyn HasFixtures<'proj>,
        fixture: &Fixture,
    ) -> Option<FixtureGetResult> {
        if let Some(fixture) = self
            .collection
            .get_fixture(py, fixture.name().function_name())
        {
            // We have already called this fixture. So we can return.
            return Some(fixture);
        }

        // To ensure we can call the current fixture, we must first look at all of its dependencies,
        // and resolve them first.
        let current_dependencies = fixture.required_fixtures(py);

        // We need to get all of the fixtures in the current scope.
        let current_all_fixtures = current.all_fixtures(None);

        for dependency in &current_dependencies {
            let mut found = false;
            for dep_fixture in &current_all_fixtures {
                if dep_fixture.name().function_name() == dependency {
                    // Avoid infinite recursion by not processing the same fixture we're currently on
                    if dep_fixture.name() != fixture.name() {
                        self.ensure_fixture_dependencies(py, parents, current, dep_fixture);
                        found = true;
                        break;
                    }
                }
            }

            // We did not find the dependency in the current scope.
            // So we try the parent scopes.
            if !found {
                for (parent, parents_above_current_parent) in iter_with_ancestors(parents) {
                    let parent_fixture = (*parent).get_fixture(dependency);

                    if let Some(parent_fixture) = parent_fixture {
                        if parent_fixture.name() != fixture.name() {
                            self.ensure_fixture_dependencies(
                                py,
                                &parents_above_current_parent,
                                parent,
                                parent_fixture,
                            );
                            break;
                        }
                    }
                }
            }
        }

        let module = current.fixture_module()?;

        match fixture.call(py, self, module, parents) {
            Ok(fixture_return) => {
                self.insert_fixture(fixture_return.clone(), fixture);

                Some(fixture_return)
            }
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);

                None
            }
        }
    }

    /// Add fixtures with the current scope to the fixture manager.
    ///
    /// This will ensure that all of the dependencies of the given fixtures are called first.
    pub(crate) fn get_fixture<'proj>(
        &mut self,
        py: Python<'_>,
        parents: &[&'proj DiscoveredPackage],
        current: &'proj dyn HasFixtures<'proj>,
        fixture_name: &str,
    ) -> Option<FixtureGetResult> {
        if let Some(fixture_return) = get_builtin_fixture(py, fixture_name) {
            return Some(fixture_return);
        }
        let fixture = current.get_fixture(fixture_name);

        if let Some(fixture_return) = fixture
            .and_then(|fixture| self.ensure_fixture_dependencies(py, parents, current, fixture))
        {
            return Some(fixture_return);
        }

        for (current, parents) in iter_with_ancestors(parents) {
            let fixture = current.get_fixture(fixture_name);

            if let Some(fixture_return) = fixture.and_then(|fixture| {
                self.ensure_fixture_dependencies(py, &parents, current, fixture)
            }) {
                return Some(fixture_return);
            }
        }

        None
    }

    /// Clears all fixtures and returns finalizers for cleanup.
    ///
    /// This method is called when a scope ends to ensure proper cleanup
    /// of resources allocated by fixtures.
    pub(crate) fn clear_finalizers(&mut self, scope: FixtureScope) -> Finalizers {
        self.collection.clear_finalizers(scope)
    }

    pub(crate) fn clear_fixtures(&mut self, scope: FixtureScope) {
        self.collection.clear_fixtures(scope);
    }

    pub(crate) fn setup_auto_use_fixtures<'proj>(
        &mut self,
        py: Python<'_>,
        parents: &[&'proj DiscoveredPackage],
        current: &'proj dyn HasFixtures<'proj>,
        scopes: &[FixtureScope],
    ) {
        let auto_use_fixtures = current.auto_use_fixtures(scopes);

        for fixture in auto_use_fixtures {
            if self
                .ensure_fixture_dependencies(py, parents, current, fixture)
                .is_some()
            {
                break;
            }
        }

        for (current, parents) in iter_with_ancestors(parents) {
            let auto_use_fixtures = current.auto_use_fixtures(scopes);

            for fixture in auto_use_fixtures {
                if self
                    .ensure_fixture_dependencies(py, &parents, current, fixture)
                    .is_some()
                {
                    break;
                }
            }
        }
    }
}
