//! Runtime fixture graph lookup, normalization, and structural validation.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

use crate::discovery::{DiscoveredPackage, DiscoveredTestFunction};
use crate::extensions::fixtures::{
    DiscoveredFixture, FixtureScope, HasFixtures, NormalizedFixture, RejectedFixture,
    RequiresFixtures, get_auto_use_fixtures,
};

/// Resolves fixtures at runtime during test execution.
///
/// Unlike pre-normalization, this resolver finds and normalizes fixtures
/// on-demand when tests need them. `current` is typed as a trait object so
/// callers may pass either a test module (normal test / module-autouse
/// resolution), a package (package-autouse resolution), or the session package
/// itself (session-autouse resolution). Package providers expose both user and
/// framework fixtures through their `HasFixtures` implementation.
pub(super) struct RuntimeFixtureResolver<'a> {
    /// Package chain searched from session root toward the current module.
    parents: &'a [&'a DiscoveredPackage],
    /// Fixture provider at the active package, module, or session scope.
    current: &'a (dyn HasFixtures<'a> + 'a),
    /// Normalized non-function fixtures reused within this resolution pass.
    fixture_cache: HashMap<String, Rc<NormalizedFixture>>,
}

/// Source-backed fixture metadata retained for resolution diagnostics.
pub struct FixtureResolutionEntry {
    /// Fixture function name.
    pub(crate) name: String,
    /// Declared lifetime scope.
    pub(crate) scope: FixtureScope,
    /// Fixture definition used to locate the diagnostic.
    pub(crate) stmt_function_def: Rc<StmtFunctionDef>,
    /// Source containing the fixture definition.
    pub(crate) source_file: SourceFile,
}

/// Structural failure discovered while resolving a fixture graph.
pub enum FixtureResolutionError {
    /// Dependency graph revisits an active fixture.
    Cycle {
        /// Ordered cycle path, including the repeated final entry.
        cycle: Vec<FixtureResolutionEntry>,
    },
    /// Narrower-lived fixture is requested by a broader-lived fixture.
    ScopeMismatch {
        /// Fixtures traversed before reaching the incompatible edge.
        dependency_path: Vec<FixtureResolutionEntry>,
        /// Fixture declaring the incompatible dependency.
        fixture: FixtureResolutionEntry,
        /// Dependency whose scope cannot satisfy its consumer.
        dependency: FixtureResolutionEntry,
    },
    /// Fixture requires names that cannot be resolved.
    MissingFixtures {
        /// Fixture declaring the missing dependencies.
        fixture: FixtureResolutionEntry,
        /// Unresolved dependency names.
        missing_fixtures: Vec<String>,
        /// Missing dependencies that were rejected during discovery.
        rejected_fixtures: Vec<RejectedFixture>,
    },
    /// Test requires names that cannot be resolved.
    MissingTestFixtures {
        /// Test definition used to locate the diagnostic.
        stmt_function_def: Rc<StmtFunctionDef>,
        /// Source containing the test definition.
        source_file: SourceFile,
        /// Unresolved fixture names.
        missing_fixtures: Vec<String>,
    },
}

/// Result returned while resolving a fixture graph.
pub(super) type FixtureResolutionResult<T> = Result<T, FixtureResolutionError>;

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
    /// Active DFS stack used to identify the exact cycle segment.
    fixtures: Vec<&'a DiscoveredFixture>,
}

impl<'a> FixturePath<'a> {
    /// Runs one recursive resolution step while maintaining the DFS stack.
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
    /// Creates a resolver for one current fixture provider and package chain.
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

    /// Normalizes a fixture and its dependencies recursively.
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

    /// Returns normalized auto-use fixtures for a given scope.
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

    /// Resolves test dependencies, excluding names supplied by parametrization.
    pub(super) fn resolve_test_fixtures(
        &mut self,
        py: Python,
        test: &DiscoveredTestFunction,
        parametrize_param_names: &HashSet<&str>,
    ) -> FixtureResolutionResult<Vec<Rc<NormalizedFixture>>> {
        let fixture_names = test.statement().required_fixtures(py);
        let regular_fixture_names: Vec<String> = fixture_names
            .iter()
            .filter(|name| !parametrize_param_names.contains(name.as_str()))
            .cloned()
            .collect();

        // Wrapped and Hypothesis decorators can supply arguments declared in source.
        let decorator_supplies_arguments = test.py_function.getattr(py, "__wrapped__").is_ok()
            || test.py_function.getattr(py, "hypothesis").is_ok();
        let missing_fixtures = if decorator_supplies_arguments {
            Vec::new()
        } else {
            regular_fixture_names
                .iter()
                .filter(|name| find_fixture(None, name, self.parents, self.current).is_none())
                .cloned()
                .collect::<Vec<_>>()
        };

        if !missing_fixtures.is_empty() {
            return Err(FixtureResolutionError::MissingTestFixtures {
                stmt_function_def: Rc::clone(test.definition().statement_rc()),
                source_file: test.source_file().clone(),
                missing_fixtures,
            });
        }

        let mut path = FixturePath::default();
        self.get_dependent_fixtures(py, None, &regular_fixture_names, &mut path)
    }

    /// Resolves fixtures requested only for their side effects.
    pub(super) fn resolve_use_fixtures(
        &mut self,
        py: Python,
        fixture_names: &[String],
    ) -> FixtureResolutionResult<Vec<Rc<NormalizedFixture>>> {
        let mut path = FixturePath::default();
        self.get_dependent_fixtures(py, None, fixture_names, &mut path)
    }

    /// Resolves a list of names and validates every dependency scope edge.
    fn get_dependent_fixtures(
        &mut self,
        py: Python,
        current_fixture: Option<&'a DiscoveredFixture>,
        fixture_names: &[String],
        path: &mut FixturePath<'a>,
    ) -> FixtureResolutionResult<Vec<Rc<NormalizedFixture>>> {
        let mut normalized_fixtures = Vec::with_capacity(fixture_names.len());
        let mut missing_fixtures = Vec::new();
        let mut rejected_fixtures = Vec::new();

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
            } else if let Some(fixture) = current_fixture {
                if fixture.name().function_name() == dep_name {
                    let normalized = self.normalize_fixture(py, fixture, path)?;
                    normalized_fixtures.push(normalized);
                } else {
                    if let Some(rejected_fixture) =
                        find_rejected_fixture(dep_name, self.parents, self.current)
                    {
                        rejected_fixtures.push(rejected_fixture.clone());
                    }
                    missing_fixtures.push(dep_name.clone());
                }
            }
        }

        if let Some(fixture) = current_fixture
            && !missing_fixtures.is_empty()
        {
            return Err(FixtureResolutionError::MissingFixtures {
                fixture: FixtureResolutionEntry::new(fixture),
                missing_fixtures,
                rejected_fixtures,
            });
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
    if current.get_rejected_fixture(name).is_some() {
        return None;
    }

    for parent in parents {
        if let Some(fixture) = parent.get_fixture(name)
            && current_fixture
                .is_none_or(|current_fixture| current_fixture.name() != fixture.name())
        {
            return Some(fixture);
        }
        if parent.get_rejected_fixture(name).is_some() {
            return None;
        }
    }

    None
}

/// Finds rejected fixture metadata using the same provider precedence as fixture lookup.
fn find_rejected_fixture<'a>(
    name: &str,
    parents: &'a [&'a DiscoveredPackage],
    current: &'a (dyn HasFixtures<'a> + 'a),
) -> Option<&'a RejectedFixture> {
    if let Some(fixture) = current.get_rejected_fixture(name) {
        return Some(fixture);
    }

    parents
        .iter()
        .find_map(|parent| parent.get_rejected_fixture(name))
}
