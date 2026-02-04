use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use pyo3::prelude::*;

use crate::discovery::{DiscoveredModule, DiscoveredPackage};
use crate::extensions::fixtures::{
    DiscoveredFixture, FixtureScope, HasFixtures, NormalizedFixture, RequiresFixtures,
    UserDefinedFixture, get_auto_use_fixtures, get_builtin_fixture,
};
use crate::utils::iter_with_ancestors;

/// Resolves fixtures at runtime during test execution.
///
/// Unlike pre-normalization, this resolver finds and normalizes fixtures
/// on-demand when tests need them.
pub(super) struct RuntimeFixtureResolver<'a> {
    parents: &'a [&'a DiscoveredPackage],
    current_module: &'a DiscoveredModule,
    fixture_cache: HashMap<String, Rc<NormalizedFixture>>,
}

impl<'a> RuntimeFixtureResolver<'a> {
    pub(super) fn new(
        parents: &'a [&'a DiscoveredPackage],
        current_module: &'a DiscoveredModule,
    ) -> Self {
        Self {
            parents,
            current_module,
            fixture_cache: HashMap::new(),
        }
    }

    /// Normalize a fixture and its dependencies recursively.
    fn normalize_fixture(
        &mut self,
        py: Python,
        fixture: &DiscoveredFixture,
    ) -> Rc<NormalizedFixture> {
        let cache_key = fixture.name().to_string();

        if let Some(cached) = self.fixture_cache.get(&cache_key) {
            return Rc::clone(cached);
        }

        let required_fixtures: Vec<String> = fixture.required_fixtures(py);
        let dependent_fixtures = self.get_dependent_fixtures(py, Some(fixture), &required_fixtures);

        let result = Rc::new(NormalizedFixture::UserDefined(UserDefinedFixture {
            name: fixture.name().clone(),
            dependencies: dependent_fixtures,
            scope: fixture.scope(),
            is_generator: fixture.is_generator(),
            py_function: Rc::new(fixture.function().clone_ref(py)),
            stmt_function_def: Rc::clone(fixture.stmt_function_def()),
        }));

        self.fixture_cache.insert(cache_key, Rc::clone(&result));

        result
    }

    /// Get normalized auto-use fixtures for a given scope.
    pub(super) fn get_normalized_auto_use_fixtures(
        &mut self,
        py: Python,
        scope: FixtureScope,
    ) -> Vec<Rc<NormalizedFixture>> {
        let auto_use_fixtures = get_auto_use_fixtures(self.parents, self.current_module, scope);

        auto_use_fixtures
            .into_iter()
            .map(|fixture| self.normalize_fixture(py, fixture))
            .collect()
    }

    /// Resolve fixture dependencies for a test, excluding parametrize params.
    pub(super) fn resolve_test_fixtures(
        &mut self,
        py: Python,
        fixture_names: &[String],
        parametrize_param_names: &HashSet<&str>,
    ) -> Vec<Rc<NormalizedFixture>> {
        let regular_fixture_names: Vec<String> = fixture_names
            .iter()
            .filter(|name| !parametrize_param_names.contains(name.as_str()))
            .cloned()
            .collect();

        self.get_dependent_fixtures(py, None, &regular_fixture_names)
    }

    /// Resolve `use_fixtures` dependencies.
    pub(super) fn resolve_use_fixtures(
        &mut self,
        py: Python,
        fixture_names: &[String],
    ) -> Vec<Rc<NormalizedFixture>> {
        self.get_dependent_fixtures(py, None, fixture_names)
    }

    /// Get dependent fixtures for a list of fixture names.
    fn get_dependent_fixtures(
        &mut self,
        py: Python,
        current_fixture: Option<&DiscoveredFixture>,
        fixture_names: &[String],
    ) -> Vec<Rc<NormalizedFixture>> {
        let mut normalized_fixtures = Vec::with_capacity(fixture_names.len());

        for dep_name in fixture_names {
            if let Some(builtin_fixture) = get_builtin_fixture(py, dep_name) {
                normalized_fixtures.push(Rc::new(builtin_fixture));
            } else if let Some(fixture) =
                find_fixture(current_fixture, dep_name, self.parents, self.current_module)
            {
                let normalized = self.normalize_fixture(py, fixture);
                normalized_fixtures.push(normalized);
            }
        }

        normalized_fixtures
    }
}

/// Finds a fixture by name, searching in the current module and parent packages.
/// We pass in the current fixture to avoid returning it (which would cause infinite recursion).
fn find_fixture<'a>(
    current_fixture: Option<&DiscoveredFixture>,
    name: &str,
    parents: &'a [&'a DiscoveredPackage],
    current: &'a DiscoveredModule,
) -> Option<&'a DiscoveredFixture> {
    if let Some(fixture) = current.get_fixture(name)
        && current_fixture.is_none_or(|current_fixture| current_fixture.name() != fixture.name())
    {
        return Some(fixture);
    }

    for (parent, _ancestors) in iter_with_ancestors(parents) {
        if let Some(fixture) = parent.get_fixture(name)
            && current_fixture
                .is_none_or(|current_fixture| current_fixture.name() != fixture.name())
        {
            return Some(fixture);
        }
    }

    None
}
