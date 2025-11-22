use indexmap::IndexMap;
use pyo3::prelude::*;

use crate::{
    Context,
    diagnostic::Diagnostic,
    discovery::DiscoveredPackage,
    extensions::fixtures::{
        Finalizer, Finalizers, Fixture, FixtureScope, HasFixtures, NormalizedFixture,
        builtins::get_builtin_fixture,
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
    fixtures: IndexMap<FixtureKey, Fixture>,

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

    pub(crate) fn insert_fixture(&mut self, fixture: Fixture) {
        self.fixtures.insert(
            FixtureKey {
                name: fixture.name().clone(),
                scope: fixture.scope(),
                auto_use: fixture.auto_use(),
            },
            fixture,
        );
    }

    fn get_fixture<'proj>(
        &mut self,
        py: Python<'_>,
        context: &mut Context,
        parents: &[&'proj DiscoveredPackage],
        current: &'proj dyn HasFixtures<'proj>,
        fixture_name: &str,
        ignore_fixtures: &[String],
    ) -> Option<&'proj Fixture> {
        // if let Some(fixture_return) = self.get_exact_fixture(fixture_name) {
        //     return Some(fixture_return);
        // }

        let fixture = current.get_fixture(fixture_name);

        if let Some(fixture) = fixture {
            return Some(fixture);
        }

        for (current, parents) in iter_with_ancestors(parents) {
            let fixture = current.get_fixture(fixture_name);

            if let Some(fixture) = fixture {
                return Some(fixture);
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

    pub(crate) fn get_auto_use_fixtures<'proj>(
        &mut self,
        parents: &'proj [&'proj DiscoveredPackage],
        current: &'proj dyn HasFixtures<'proj>,
        scopes: &[FixtureScope],
        ignore_fixtures: &[String],
    ) -> Vec<&'proj Fixture> {
        let mut auto_use_fixtures_called = Vec::new();
        let auto_use_fixtures = current.auto_use_fixtures(scopes);

        for fixture in auto_use_fixtures {
            let fixture_name = fixture.name().function_name().to_string();

            if ignore_fixtures.contains(&fixture_name) {
                continue;
            }

            if auto_use_fixtures_called
                .iter()
                .any(|fixture: &&Fixture| fixture.name().function_name() == fixture_name)
            {
                continue;
            }

            auto_use_fixtures_called.push(fixture);
            break;
        }

        for parent in parents {
            let auto_use_fixtures = parent.auto_use_fixtures(scopes);

            for fixture in auto_use_fixtures {
                let fixture_name = fixture.name().function_name().to_string();

                if ignore_fixtures.contains(&fixture_name) {
                    continue;
                }

                if auto_use_fixtures_called
                    .iter()
                    .any(|fixture| fixture.name().function_name() == fixture_name)
                {
                    continue;
                }

                auto_use_fixtures_called.push(fixture);
                break;
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
    fn get_exact_fixture(&self, fixture_name: &str) -> Option<Fixture> {
        if let Some((_, fixture)) = self
            .fixtures
            .iter()
            .rev()
            .find(|(key, _)| key.name.function_name() == fixture_name)
        {
            return Some(fixture.clone());
        }

        None
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
