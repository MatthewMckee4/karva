//! Runtime fixture graph lookup, normalization, and structural validation.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use camino::Utf8Path;
use pyo3::prelude::*;

use crate::discovery::models::definition::{FunctionDefinition, TestDefinition};
use crate::discovery::{DiscoveredPackage, DiscoveredTestFunction};
use crate::extensions::fixtures::{
    DiscoveredFixture, FixtureId, FixturePlan, FixtureScope, HasFixtures, NormalizedFixture,
    RejectedFixture, RequiresFixtures, get_auto_use_fixtures,
};

/// Compiles fixture lookup results into an arena for one request context.
///
/// Unlike pre-normalization, this resolver finds and normalizes fixtures
/// on-demand when tests need them. `current` is typed as a trait object so
/// callers may pass either a test module (normal test / module-autouse
/// resolution), a package (package-autouse resolution), or the session package
/// itself (session-autouse resolution). Package providers expose both user and
/// framework fixtures through their `HasFixtures` implementation.
pub(super) struct FixturePlanCompiler<'a> {
    /// Package chain searched from session root toward the current module.
    parents: &'a [&'a DiscoveredPackage],
    /// Fixture provider at the active package, module, or session scope.
    current: &'a (dyn HasFixtures<'a> + 'a),
    /// Package owning fixtures provided by `current`.
    current_package: &'a Utf8Path,
    /// Definition IDs reused within this compilation pass.
    fixture_ids: HashMap<String, FixtureId>,

    /// Arena under construction.
    fixtures: Vec<NormalizedFixture>,
}

/// Source-backed fixture metadata retained for resolution diagnostics.
pub struct FixtureResolutionEntry {
    /// Fixture function name.
    pub(crate) name: String,
    /// Declared lifetime scope.
    pub(crate) scope: FixtureScope,
    /// Immutable fixture identity, syntax, and source.
    pub(crate) definition: Rc<FunctionDefinition>,
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
        /// Immutable test identity and source.
        definition: Rc<TestDefinition>,
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
            definition: Rc::clone(fixture.definition()),
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

impl<'a> FixturePlanCompiler<'a> {
    /// Creates a resolver for one current fixture provider and package chain.
    pub(super) fn new(
        parents: &'a [&'a DiscoveredPackage],
        current: &'a (dyn HasFixtures<'a> + 'a),
        current_package: &'a Utf8Path,
    ) -> Self {
        Self {
            parents,
            current,
            current_package,
            fixture_ids: HashMap::new(),
            fixtures: Vec::new(),
        }
    }

    /// Finds the package whose provider contributed `fixture`.
    fn package_owner(&self, fixture: &DiscoveredFixture) -> &Utf8Path {
        let name = fixture.name().function_name();
        if self
            .current
            .get_fixture(name)
            .is_some_and(|candidate| std::ptr::eq(candidate, fixture))
        {
            return self.current_package;
        }

        self.parents
            .iter()
            .find(|package| {
                package
                    .get_fixture(name)
                    .is_some_and(|candidate| std::ptr::eq(candidate, fixture))
            })
            .map_or(self.current_package, |package| package.path())
    }

    /// Finishes the immutable arena after all requested root groups are compiled.
    pub(super) fn finish(self) -> FixturePlan {
        FixturePlan::new(self.fixtures)
    }

    /// Normalizes a fixture and its dependencies recursively.
    ///
    /// A definition is stored once per request context. Runtime scope caches still
    /// decide when its Python value is reused.
    fn normalize_fixture(
        &mut self,
        py: Python,
        fixture: &'a DiscoveredFixture,
        path: &mut FixturePath<'a>,
    ) -> FixtureResolutionResult<FixtureId> {
        let cache_key = fixture.name().to_string();

        if let Some(cached) = self.fixture_ids.get(&cache_key) {
            return Ok(*cached);
        }

        let dependent_fixtures = path.enter(fixture, |path| {
            let required_fixtures: Vec<String> = fixture.required_fixtures(py);
            self.get_dependent_fixtures(py, Some(fixture), &required_fixtures, path)
        })?;

        let result = NormalizedFixture {
            definition: Rc::clone(fixture.definition()),
            dependencies: dependent_fixtures,
            scope: fixture.scope(),
            package_owner: self.package_owner(fixture).to_path_buf(),
            is_generator: fixture.is_generator(),
            py_function: fixture.function().clone_ref(py),
        };

        let fixture_id = FixtureId::new(self.fixtures.len());
        self.fixtures.push(result);
        self.fixture_ids.insert(cache_key, fixture_id);

        Ok(fixture_id)
    }

    /// Returns normalized auto-use fixtures for a given scope.
    pub(super) fn get_normalized_auto_use_fixtures(
        &mut self,
        py: Python,
        scope: FixtureScope,
    ) -> FixtureResolutionResult<Vec<FixtureId>> {
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
    ) -> FixtureResolutionResult<Vec<FixtureId>> {
        let fixture_names = test.required_fixtures();
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
                definition: Rc::clone(test.definition()),
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
    ) -> FixtureResolutionResult<Vec<FixtureId>> {
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
    ) -> FixtureResolutionResult<Vec<FixtureId>> {
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
