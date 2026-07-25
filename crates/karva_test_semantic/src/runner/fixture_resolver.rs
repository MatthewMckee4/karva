use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

use crate::discovery::DiscoveredPackage;
use crate::extensions::fixtures::{
    DiscoveredFixture, FixtureScope, HasFixtures, NormalizedFixture, RequiresFixtures,
    get_auto_use_fixtures,
};

/// Resolves fixtures at runtime during test execution.
///
/// Unlike pre-normalization, this resolver finds and normalizes fixtures
/// on-demand when tests need them. `current` is typed as a trait object so
/// callers may pass either a test module (normal test / module-autouse
/// resolution), a conftest module (package-autouse resolution), or the
/// session package itself (session-autouse resolution) — the latter gives
/// session-level autouse fixtures visibility into `framework_module` via
/// the `HasFixtures` impl on `DiscoveredPackage`.
pub(super) struct RuntimeFixtureResolver<'a> {
    parents: &'a [&'a DiscoveredPackage],
    current: &'a (dyn HasFixtures<'a> + 'a),
    fixture_cache: HashMap<String, Rc<NormalizedFixture>>,
}

pub struct FixtureResolutionEntry {
    pub(crate) name: String,
    pub(crate) scope: FixtureScope,
    pub(crate) stmt_function_def: Rc<StmtFunctionDef>,
    pub(crate) source_file: SourceFile,
}

pub enum FixtureResolutionError {
    Cycle {
        cycle: Vec<FixtureResolutionEntry>,
    },
    ScopeMismatch {
        dependency_path: Vec<FixtureResolutionEntry>,
        fixture: FixtureResolutionEntry,
        dependency: FixtureResolutionEntry,
    },
}

pub type FixtureResolutionResult<T> = Result<T, FixtureResolutionError>;

impl FixtureResolutionEntry {
    fn new(fixture: &DiscoveredFixture) -> Self {
        Self {
            name: fixture.name().function_name().to_string(),
            scope: fixture.scope(),
            stmt_function_def: Rc::clone(fixture.stmt_function_def()),
            source_file: fixture.source_file().clone(),
        }
    }
}

impl FixtureResolutionError {
    fn cycle(cycle: &[&DiscoveredFixture], repeated: &DiscoveredFixture) -> Self {
        let cycle = cycle
            .iter()
            .copied()
            .chain(std::iter::once(repeated))
            .map(FixtureResolutionEntry::new)
            .collect();

        Self::Cycle { cycle }
    }

    fn scope_mismatch(
        dependency_path: &[&DiscoveredFixture],
        fixture: &DiscoveredFixture,
        dependency: &DiscoveredFixture,
    ) -> Self {
        let dependency_path = dependency_path
            .iter()
            .copied()
            .map(FixtureResolutionEntry::new)
            .collect();

        Self::ScopeMismatch {
            dependency_path,
            fixture: FixtureResolutionEntry::new(fixture),
            dependency: FixtureResolutionEntry::new(dependency),
        }
    }
}

#[derive(Default)]
struct FixturePath<'a> {
    fixtures: Vec<&'a DiscoveredFixture>,
}

impl<'a> FixturePath<'a> {
    fn enter<T>(
        &mut self,
        fixture: &'a DiscoveredFixture,
        resolve: impl FnOnce(&mut Self) -> FixtureResolutionResult<T>,
    ) -> FixtureResolutionResult<T> {
        if let Some(cycle_start) = self
            .fixtures
            .iter()
            .position(|active_fixture| std::ptr::eq(*active_fixture, fixture))
        {
            return Err(FixtureResolutionError::cycle(
                &self.fixtures[cycle_start..],
                fixture,
            ));
        }

        self.fixtures.push(fixture);
        let result = resolve(self);
        let _ = self.fixtures.pop();
        result
    }
}

impl<'a> RuntimeFixtureResolver<'a> {
    pub(super) fn new(
        parents: &'a [&'a DiscoveredPackage],
        current: &'a (dyn HasFixtures<'a> + 'a),
    ) -> Self {
        Self {
            parents,
            current,
            fixture_cache: HashMap::new(),
        }
    }

    /// Normalize a fixture and its dependencies recursively.
    ///
    /// Function-scoped fixtures are NOT cached because their built-in dependencies
    /// (e.g. `tmp_path`) must be fresh for each test invocation. Broader-scoped
    /// fixtures are cached so they are shared across tests within the appropriate
    /// scope.
    fn normalize_fixture(
        &mut self,
        py: Python,
        fixture: &'a DiscoveredFixture,
        path: &mut FixturePath<'a>,
    ) -> FixtureResolutionResult<Rc<NormalizedFixture>> {
        let cache_key = fixture.name().to_string();

        if fixture.scope() != FixtureScope::Function {
            if let Some(cached) = self.fixture_cache.get(&cache_key) {
                return Ok(Rc::clone(cached));
            }
        }

        let dependent_fixtures = path.enter(fixture, |path| {
            let required_fixtures: Vec<String> = fixture.required_fixtures(py);
            self.get_dependent_fixtures(py, Some(fixture), &required_fixtures, path)
        })?;

        let result = Rc::new(NormalizedFixture {
            name: fixture.name().clone(),
            dependencies: dependent_fixtures,
            scope: fixture.scope(),
            is_generator: fixture.is_generator(),
            py_function: Rc::new(fixture.function().clone_ref(py)),
            stmt_function_def: Rc::clone(fixture.stmt_function_def()),
            source_file: fixture.source_file().clone(),
        });

        if fixture.scope() != FixtureScope::Function {
            self.fixture_cache.insert(cache_key, Rc::clone(&result));
        }

        Ok(result)
    }

    /// Get normalized auto-use fixtures for a given scope.
    pub(super) fn get_normalized_auto_use_fixtures(
        &mut self,
        py: Python,
        scope: FixtureScope,
    ) -> FixtureResolutionResult<Vec<Rc<NormalizedFixture>>> {
        let auto_use_fixtures = get_auto_use_fixtures(self.parents, self.current, scope);
        let mut path = FixturePath::default();

        auto_use_fixtures
            .into_iter()
            .map(|fixture| self.normalize_fixture(py, fixture, &mut path))
            .collect()
    }

    /// Resolve fixture dependencies for a test, excluding parametrize params.
    pub(super) fn resolve_test_fixtures(
        &mut self,
        py: Python,
        fixture_names: &[String],
        parametrize_param_names: &HashSet<&str>,
    ) -> FixtureResolutionResult<Vec<Rc<NormalizedFixture>>> {
        let regular_fixture_names: Vec<String> = fixture_names
            .iter()
            .filter(|name| !parametrize_param_names.contains(name.as_str()))
            .cloned()
            .collect();

        let mut path = FixturePath::default();
        self.get_dependent_fixtures(py, None, &regular_fixture_names, &mut path)
    }

    /// Resolve `use_fixtures` dependencies.
    pub(super) fn resolve_use_fixtures(
        &mut self,
        py: Python,
        fixture_names: &[String],
    ) -> FixtureResolutionResult<Vec<Rc<NormalizedFixture>>> {
        let mut path = FixturePath::default();
        self.get_dependent_fixtures(py, None, fixture_names, &mut path)
    }

    /// Get dependent fixtures for a list of fixture names.
    fn get_dependent_fixtures(
        &mut self,
        py: Python,
        current_fixture: Option<&'a DiscoveredFixture>,
        fixture_names: &[String],
        path: &mut FixturePath<'a>,
    ) -> FixtureResolutionResult<Vec<Rc<NormalizedFixture>>> {
        let mut normalized_fixtures = Vec::with_capacity(fixture_names.len());

        for dep_name in fixture_names {
            if let Some(fixture) =
                find_fixture(current_fixture, dep_name, self.parents, self.current)
            {
                if let Some(current_fixture) = current_fixture
                    && !current_fixture.scope().can_use(fixture.scope())
                {
                    let dependency_path = path
                        .fixtures
                        .split_last()
                        .map_or(&[][..], |(_, dependency_path)| dependency_path);
                    return Err(FixtureResolutionError::scope_mismatch(
                        dependency_path,
                        current_fixture,
                        fixture,
                    ));
                }
                let normalized = self.normalize_fixture(py, fixture, path)?;
                normalized_fixtures.push(normalized);
            } else if let Some(fixture) = current_fixture
                && fixture.name().function_name() == dep_name
            {
                let normalized = self.normalize_fixture(py, fixture, path)?;
                normalized_fixtures.push(normalized);
            }
        }

        Ok(normalized_fixtures)
    }
}

/// Finds a fixture by name, searching in the current node and parent packages.
/// The current definition is skipped so a fixture can override and depend on a
/// same-name fixture from a parent scope. If no override exists, the resolver
/// handles the dependency as a direct cycle.
fn find_fixture<'a>(
    current_fixture: Option<&DiscoveredFixture>,
    name: &str,
    parents: &'a [&'a DiscoveredPackage],
    current: &'a (dyn HasFixtures<'a> + 'a),
) -> Option<&'a DiscoveredFixture> {
    if let Some(fixture) = current.get_fixture(name)
        && current_fixture.is_none_or(|current_fixture| current_fixture.name() != fixture.name())
    {
        return Some(fixture);
    }

    for parent in parents {
        if let Some(fixture) = parent.get_fixture(name)
            && current_fixture
                .is_none_or(|current_fixture| current_fixture.name() != fixture.name())
        {
            return Some(fixture);
        }
    }

    None
}
