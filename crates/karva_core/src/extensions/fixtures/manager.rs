use indexmap::IndexMap;

use crate::{
    diagnostic::Diagnostic,
    extensions::fixtures::{Finalizer, Fixture, FixtureScope, HasFixtures},
    name::QualifiedFunctionName,
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

    pub(crate) fn clear_fixtures(&mut self, scope: FixtureScope) {
        self.fixtures.retain(|key, _| key.scope != scope);
    }

    pub(crate) fn new_finalizer_scope(&mut self) {
        let new_finalizer = vec![];
        self.finalizers.push(new_finalizer);
    }
}

pub(crate) fn get_auto_use_fixtures<'proj>(
    current: &'proj dyn HasFixtures<'proj>,
    scopes: &[FixtureScope],
) -> Vec<&'proj Fixture> {
    let mut auto_use_fixtures_called = Vec::new();
    let auto_use_fixtures = current.auto_use_fixtures(scopes);

    for fixture in auto_use_fixtures {
        let fixture_name = fixture.name().function_name().to_string();

        if auto_use_fixtures_called
            .iter()
            .any(|fixture: &&Fixture| fixture.name().function_name() == fixture_name)
        {
            continue;
        }

        auto_use_fixtures_called.push(fixture);
        break;
    }

    auto_use_fixtures_called
}
