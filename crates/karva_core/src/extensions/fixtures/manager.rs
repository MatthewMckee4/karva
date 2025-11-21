use indexmap::IndexMap;
use pyo3::prelude::*;

use crate::{
    Context,
    diagnostic::Diagnostic,
    discovery::DiscoveredPackage,
    extensions::fixtures::{
        Finalizer, Finalizers, Fixture, FixtureScope, HasFixtures, NormalizedFixture, builtins::get_builtin_fixture,
    },
    name::QualifiedFunctionName,
    utils::iter_with_ancestors,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct FixtureKey {
    pub(crate) name: QualifiedFunctionName,
    pub(crate) scope: FixtureScope,
    pub(crate) auto_use: bool,
}

#[derive(Debug)]
pub(crate) struct FixtureManager {
    /// Map of fixture names to their resolved Python values
    fixtures: IndexMap<FixtureKey, Vec<NormalizedFixture>>,

    /// A stack of finalizers.
    ///
    /// Each time a cleanup is run, we pop the finalizers from the stack and run them.
    finalizers: Vec<Vec<Finalizer>>,

    /// The diagnostics from creating fixtures
    diagnostics: Vec<Diagnostic>,
}

impl FixtureManager {
    pub(crate) fn new() -> Self {
        Self {
            fixtures: IndexMap::default(),
            finalizers: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub(crate) fn clear_diagnostics(&mut self) -> Vec<Diagnostic> {
        self.diagnostics.drain(..).collect()
    }

    pub(crate) fn insert_fixture(&mut self, fixture_return: Vec<NormalizedFixture>, fixture: &Fixture) {
        self.fixtures.insert(
            FixtureKey {
                name: fixture.name().clone(),
                scope: fixture.scope(),
                auto_use: fixture.auto_use(),
            },
            fixture_return,
        );
    }

    /// Recursively resolves and executes fixture dependencies.
    ///
    /// This method ensures that all dependencies of a fixture are resolved and executed
    /// before the fixture itself is called. It performs a depth-first traversal of the
    /// dependency graph, checking both the current scope and parent scopes for required fixtures.
    ///
    /// TODO: This method needs to be rewritten to work with the new normalization approach.
    /// For now, just stub it out to make the code compile.
    fn get_normalized_fixture<'proj>(
        &mut self,
        _py: Python<'_>,
        _context: &mut Context,
        _parents: &[&'proj DiscoveredPackage],
        _current: &'proj dyn HasFixtures<'proj>,
        _fixture: &Fixture,
        _ignore_fixtures: &[String],
    ) -> Option<Vec<NormalizedFixture>> {
        // TODO: This code needs to be updated to work with the new normalization approach
        // For now, just return None to make the code compile
        // The normalization is now handled by DiscoveredPackageNormalizer
        None
    }

    /// Add fixtures with the current scope to the fixture manager.
    ///
    /// This will ensure that all of the dependencies of the given fixtures are called first.
    ///
    /// TODO: This method needs to be updated to work with Vec<NormalizedFixture>
    /// For now, just stub it out to make the code compile
    pub(crate) fn get_fixture<'proj>(
        &mut self,
        py: Python<'_>,
        _context: &mut Context,
        _parents: &[&'proj DiscoveredPackage],
        _current: &'proj dyn HasFixtures<'proj>,
        fixture_name: &str,
        _ignore_fixtures: &[String],
    ) -> Option<Vec<NormalizedFixture>> {
        // For built-in fixtures, return them in a Vec
        if let Some(fixture_return) = get_builtin_fixture(py, fixture_name) {
            return Some(vec![fixture_return]);
        }

        // TODO: Implement proper fixture resolution using the new normalization approach
        None
    }

    #[allow(dead_code)]
    fn get_fixture_old<'proj>(
        &mut self,
        py: Python<'_>,
        context: &mut Context,
        parents: &[&'proj DiscoveredPackage],
        current: &'proj dyn HasFixtures<'proj>,
        fixture_name: &str,
        ignore_fixtures: &[String],
    ) -> Option<Vec<NormalizedFixture>> {
        if let Some(fixture_return) = get_builtin_fixture(py, fixture_name) {
            return Some(vec![fixture_return]);
        }
        let fixture = current.get_fixture(fixture_name);

        if let Some(fixture_return) = fixture.and_then(|fixture| {
            self.get_normalized_fixture(py, context, parents, current, fixture, ignore_fixtures)
        }) {
            return Some(fixture_return);
        }

        for (current, parents) in iter_with_ancestors(parents) {
            let fixture = current.get_fixture(fixture_name);

            if let Some(fixture_return) = fixture.and_then(|fixture| {
                self.get_normalized_fixture(
                    py,
                    context,
                    &parents,
                    current,
                    fixture,
                    ignore_fixtures,
                )
            }) {
                return Some(fixture_return);
            }
        }

        None
    }

    pub(crate) fn clear_fixtures(&mut self, scope: FixtureScope) {
        self.fixtures.retain(|key, _| key.scope != scope);
    }

    pub(crate) fn clear_auto_use_fixtures(&mut self, auto_use_fixtures_called: &[String]) {
        self.clear_exact_fixtures(auto_use_fixtures_called);
    }

    pub(crate) fn setup_auto_use_fixtures<'proj>(
        &mut self,
        py: Python<'_>,
        context: &mut Context,
        parents: &[&'proj DiscoveredPackage],
        current: &'proj dyn HasFixtures<'proj>,
        scopes: &[FixtureScope],
        ignore_fixtures: &[String],
    ) -> Vec<String> {
        let mut auto_use_fixtures_called = ignore_fixtures.to_vec();

        let auto_use_fixtures = current.auto_use_fixtures(scopes);

        for fixture in auto_use_fixtures {
            let fixture_name = fixture.name().function_name().to_string();
            if auto_use_fixtures_called.contains(&fixture_name) {
                continue;
            }
            if self
                .get_normalized_fixture(py, context, parents, current, fixture, ignore_fixtures)
                .is_some()
            {
                auto_use_fixtures_called.push(fixture_name);
                break;
            }
        }

        for (current, parents) in iter_with_ancestors(parents) {
            let auto_use_fixtures = current.auto_use_fixtures(scopes);

            for fixture in auto_use_fixtures {
                let fixture_name = fixture.name().function_name().to_string();

                if auto_use_fixtures_called.contains(&fixture_name) {
                    continue;
                }
                if self
                    .get_normalized_fixture(
                        py,
                        context,
                        &parents,
                        current,
                        fixture,
                        ignore_fixtures,
                    )
                    .is_some()
                {
                    auto_use_fixtures_called.push(fixture_name);
                    break;
                }
            }
        }

        auto_use_fixtures_called
    }

    pub(crate) fn new_finalizer_scope(&mut self) {
        let new_finalizer = vec![];
        self.finalizers.push(new_finalizer);
    }

    pub(crate) fn insert_finalizer(&mut self, finalizer: Finalizer) {
        if let Some(last_finalizer) = self.finalizers.last_mut() {
            last_finalizer.push(finalizer);
        } else {
            let new_finalizer = vec![finalizer];
            self.finalizers.push(new_finalizer);
        }
    }

    /// Get a fixture by name
    ///
    /// If the fixture name matches a built-in fixture,
    /// it creates the fixture on-demand and stores it.
    fn get_exact_fixture(&self, py: Python<'_>, fixture_name: &str) -> Option<Vec<NormalizedFixture>> {
        if let Some((_, fixture)) = self
            .fixtures
            .iter()
            .rev()
            .find(|(key, _)| key.name.function_name() == fixture_name)
        {
            return Some(fixture.clone());
        }

        get_builtin_fixture(py, fixture_name).map(|f| vec![f])
    }

    pub(crate) fn clear_finalizers(&mut self) -> Finalizers {
        let last_finalizers = self.finalizers.pop().unwrap_or_default();
        Finalizers::new(last_finalizers)
    }

    fn clear_exact_fixtures(&mut self, fixture_names: &[String]) {
        self.fixtures
            .retain(|key, _| !fixture_names.contains(&key.name.function_name().to_string()));
    }

    pub(crate) fn remove_fixture(&mut self, name: &str) {
        self.fixtures
            .retain(|key, _| key.name.function_name() != name);
    }
}
