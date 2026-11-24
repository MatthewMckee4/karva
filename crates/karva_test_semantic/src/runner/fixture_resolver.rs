//! Runtime fixture graph lookup, normalization, and structural validation.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use camino::Utf8Path;
use karva_python_semantic::QualifiedFunctionName;
use pyo3::prelude::*;

use crate::discovery::models::definition::{
    FunctionDefinition, TestDefinition, required_keyword_parameter_names,
};
use crate::discovery::{DiscoveredPackage, DiscoveredTestFunction};
use crate::extensions::fixtures::{
    DiscoveredFixture, DynamicFixture, DynamicFixtures, FixtureContext, FixtureDependency,
    FixtureId, FixtureIdentity, FixtureLookup, FixturePlan, FixtureScope, HasFixtures,
    NormalizedFixture, RejectedFixture, get_auto_use_fixtures,
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
    /// IDs shared by fixtures whose dependencies are unchanged by parametrization.
    fixture_ids: HashMap<FixtureIdentity, FixtureId>,

    /// Parameter names and memoized resolutions for the current test.
    request: FixtureRequest,

    /// Root fixture lookup outcomes reused across validation and resolution.
    root_fixture_lookups: HashMap<String, FixtureLookup<'a>>,

    /// Arena under construction.
    fixtures: Vec<NormalizedFixture>,
    dynamic: bool,
    dynamic_parameters: HashMap<QualifiedFunctionName, FixtureParameterNames>,
    indirect_fixtures: &'a HashSet<QualifiedFunctionName>,
}

#[derive(Default)]
struct FixtureRequest {
    names: FixtureParameterNames,
    fixture_ids: HashMap<FixtureIdentity, FixtureId>,
}

#[derive(Clone, Default)]
struct FixtureParameterNames {
    direct: HashSet<String>,
    indirect: HashSet<String>,
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
    /// A broader fixture depends on a value that changes per test case.
    ParameterScopeMismatch {
        /// Fixture declaring the incompatible dependency.
        fixture: FixtureResolutionEntry,

        /// Directly parametrized argument name.
        parameter: String,
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
        indirect_fixtures: &'a HashSet<QualifiedFunctionName>,
    ) -> Self {
        Self {
            parents,
            current,
            current_package,
            fixture_ids: HashMap::new(),
            request: FixtureRequest::default(),
            root_fixture_lookups: HashMap::new(),
            fixtures: Vec::new(),
            dynamic: false,
            dynamic_parameters: HashMap::new(),
            indirect_fixtures,
        }
    }

    /// Resolves one test's roots with its parameters shadowing fixture names.
    ///
    /// Parametrized tests need their own definition IDs because the same fixture
    /// can have different edges depending on which names the test supplies.
    pub(super) fn with_parameters<T>(
        &mut self,
        direct: HashSet<String>,
        indirect: HashSet<String>,
        resolve: impl FnOnce(&mut Self) -> T,
    ) -> T {
        self.request.names = FixtureParameterNames { direct, indirect };
        let result = resolve(self);
        self.request = FixtureRequest::default();
        result
    }

    /// Looks up a fixture without a current fixture dependency context.
    fn lookup_root_fixture(&mut self, name: &str) -> FixtureLookup<'a> {
        if let Some(lookup) = self.root_fixture_lookups.get(name).copied() {
            return lookup;
        }

        let lookup = lookup_fixture(None, name, self.parents, self.current);
        self.root_fixture_lookups.insert(name.to_owned(), lookup);
        lookup
    }

    pub(super) fn root_fixture_scope(&mut self, name: &str) -> Option<FixtureScope> {
        match self.lookup_root_fixture(name) {
            FixtureLookup::Found(fixture) => Some(fixture.scope()),
            _ => None,
        }
    }

    /// Finds the package whose provider contributed `fixture`.
    fn package_owner(&self, fixture: &DiscoveredFixture) -> &Utf8Path {
        let name = fixture.name().function_name();
        if matches!(
            self.current.lookup_fixture(name),
            FixtureLookup::Found(candidate) if std::ptr::eq(candidate, fixture)
        ) {
            return self.current_package;
        }

        self.parents
            .iter()
            .rev()
            .find(|package| {
                matches!(
                    package.lookup_fixture(name),
                    FixtureLookup::Found(candidate) if std::ptr::eq(candidate, fixture)
                )
            })
            .map_or(self.current_package, |package| package.path())
    }

    /// Finishes the immutable arena after all requested root groups are compiled.
    pub(super) fn finish(mut self, py: Python<'_>) -> FixturePlan {
        let selected_len = self.fixtures.len();
        let dynamic = if self.dynamic {
            self.dynamic_fixtures(py)
        } else {
            DynamicFixtures::default()
        };

        // A request retained by a session fixture can be called from another
        // module even when the active test never asked for a request itself.
        let parametrized_dynamic = std::mem::take(&mut self.dynamic_parameters)
            .into_iter()
            .map(|(test, parameters)| {
                let overlay =
                    self.with_parameters(parameters.direct, parameters.indirect, |compiler| {
                        compiler.dynamic_fixtures(py)
                    });
                (test, overlay)
            })
            .collect();
        FixturePlan::new(self.fixtures, dynamic, parametrized_dynamic, selected_len)
    }

    fn dynamic_fixtures(&mut self, py: Python<'_>) -> DynamicFixtures {
        let mut dynamic = DynamicFixtures::default();
        let mut names = HashSet::new();
        self.current.fixture_names(&mut names);
        for parent in self.parents {
            parent.fixture_names(&mut names);
        }
        for name in names {
            let FixtureLookup::Found(fixture) = self.lookup_root_fixture(name) else {
                continue;
            };
            dynamic
                .roots
                .insert(name.to_owned(), self.compile_dynamic(py, fixture));

            let mut active = fixture;
            while let FixtureLookup::Found(parent) =
                lookup_fixture(Some(active), name, self.parents, self.current)
            {
                dynamic
                    .overrides
                    .insert(active.name().clone(), self.compile_dynamic(py, parent));
                active = parent;
            }
        }
        dynamic
    }

    fn compile_dynamic(
        &mut self,
        py: Python<'_>,
        fixture: &'a DiscoveredFixture,
    ) -> DynamicFixture {
        DynamicFixture {
            name: fixture.name().clone(),
            resolution: self
                .normalize_fixture(py, fixture, &mut FixturePath::default())
                .map_err(crate::diagnostic::fixture_resolution_diagnostic),
        }
    }

    /// Retains dynamic lookup results only for a context that injects a request.
    pub(super) fn require_request(&mut self) {
        self.dynamic = true;
    }

    /// Captures the active test's direct parameter names for lazy lookup.
    pub(super) fn retain_dynamic_parameters(&mut self, test: &QualifiedFunctionName) {
        if !self.request.names.direct.is_empty() {
            self.dynamic_parameters
                .insert(test.clone(), self.request.names.clone());
        }
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
        let parametrized = !self.request.names.direct.is_empty();
        let cached = if parametrized {
            self.request.fixture_ids.get(fixture.identity())
        } else {
            self.fixture_ids.get(fixture.identity())
        };
        if let Some(cached) = cached {
            return Ok(*cached);
        }

        let required_fixtures =
            required_keyword_parameter_names(fixture.stmt_function_def().parameters.as_ref());
        let takes_request = required_fixtures.contains(&"request");
        self.dynamic |= takes_request || fixture.parameters.is_some();
        let dependent_fixtures = path.enter(fixture, |path| {
            self.get_dependent_fixtures(py, fixture, &required_fixtures, path)
        })?;

        let uses_parameters = parametrized
            && dependent_fixtures
                .iter()
                .any(|dependency| match dependency {
                    FixtureDependency::Fixture(id) => self.fixtures[id.index()].uses_parameters,
                    FixtureDependency::Parameter(_) => true,
                });
        if parametrized
            && !uses_parameters
            && let Some(&id) = self.fixture_ids.get(fixture.identity())
        {
            self.request
                .fixture_ids
                .insert(fixture.identity().clone(), id);
            return Ok(id);
        }

        let broadest_scope = dependent_fixtures
            .iter()
            .filter_map(|dependency| match dependency {
                FixtureDependency::Fixture(id) => Some(self.fixtures[id.index()].broadest_scope),
                FixtureDependency::Parameter(_) => None,
            })
            .fold(fixture.scope(), FixtureScope::max);
        let context = if takes_request {
            FixtureContext::Request
        } else if fixture.parameters.is_some()
            || self.indirect_fixtures.contains(fixture.name())
            || dependent_fixtures
                .iter()
                .filter_map(FixtureDependency::fixture_id)
                .any(|id| self.fixture(id).needs_test_context())
        {
            FixtureContext::Test
        } else {
            FixtureContext::Independent
        };
        let result = NormalizedFixture {
            identity: fixture.identity().clone(),
            context,
            parameters: fixture.parameters.clone(),
            broadest_scope,
            dependencies: dependent_fixtures,
            uses_parameters,
            scope: fixture.scope(),
            package_owner: self.package_owner(fixture).to_path_buf(),
            is_generator: fixture.is_generator(),
            py_function: fixture.function().clone_ref(py),
        };

        let fixture_id = FixtureId::new(self.fixtures.len());
        self.fixtures.push(result);
        if !uses_parameters {
            self.fixture_ids
                .insert(fixture.identity().clone(), fixture_id);
        }
        if parametrized {
            self.request
                .fixture_ids
                .insert(fixture.identity().clone(), fixture_id);
        }

        Ok(fixture_id)
    }

    /// Finds fixture-only parameters before validation runs any fixture code.
    ///
    /// A direct parameter stops traversal at its name, even if a fixture with
    /// that name would have introduced further dependencies.
    pub(super) fn fixture_parameter_names<'test>(
        &mut self,
        test: &'test DiscoveredTestFunction,
    ) -> HashSet<&'test str> {
        let parameters: HashSet<_> = test.tags.parametrize_names().collect();
        let mut used = HashSet::new();
        let mut fixtures = Vec::new();

        for name in test
            .required_fixtures()
            .into_iter()
            .chain(test.tags.use_fixture_names())
            .chain(test.tags.indirect_fixture_names())
            .filter(|name| *name != "request")
        {
            if let Some(&parameter) = parameters.get(name) {
                used.insert(parameter);
            } else if let FixtureLookup::Found(fixture) = self.lookup_root_fixture(name) {
                fixtures.push(fixture);
            }
        }
        for fixture in get_auto_use_fixtures(self.parents, self.current, FixtureScope::Function) {
            if let Some(&parameter) = parameters.get(fixture.name().function_name()) {
                used.insert(parameter);
            } else {
                fixtures.push(fixture);
            }
        }

        let mut visited = HashSet::new();
        while let Some(fixture) = fixtures.pop() {
            if !visited.insert(std::ptr::from_ref(fixture)) {
                continue;
            }
            let names =
                required_keyword_parameter_names(fixture.stmt_function_def().parameters.as_ref());
            for name in names.into_iter().filter(|name| *name != "request") {
                if let Some(&parameter) = parameters.get(name) {
                    used.insert(parameter);
                } else if let FixtureLookup::Found(dependency) =
                    lookup_fixture(Some(fixture), name, self.parents, self.current)
                {
                    fixtures.push(dependency);
                }
            }
        }
        used
    }

    /// Returns normalized auto-use fixtures for a given scope.
    pub(super) fn get_normalized_auto_use_fixtures(
        &mut self,
        py: Python,
        scope: FixtureScope,
    ) -> FixtureResolutionResult<Vec<FixtureId>> {
        let mut auto_use_fixtures = get_auto_use_fixtures(self.parents, self.current, scope);
        auto_use_fixtures.retain(|fixture| {
            !self
                .request
                .names
                .direct
                .contains(fixture.name().function_name())
        });
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
    ) -> FixtureResolutionResult<Vec<FixtureId>> {
        let fixture_names = test.required_fixtures();
        let regular_fixture_names: Vec<&str> = fixture_names
            .iter()
            .filter(|name| **name != "request" && !self.request.names.direct.contains(**name))
            .copied()
            .collect();

        // Wrapped and Hypothesis decorators can supply arguments declared in source.
        let decorator_supplies_arguments = test.py_function.getattr(py, "__wrapped__").is_ok()
            || test.py_function.getattr(py, "hypothesis").is_ok();
        let missing_fixtures = if decorator_supplies_arguments {
            Vec::new()
        } else {
            regular_fixture_names
                .iter()
                .filter(|name| !self.lookup_root_fixture(name).is_found())
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        };

        if !missing_fixtures.is_empty() {
            return Err(FixtureResolutionError::MissingTestFixtures {
                definition: Rc::clone(test.definition()),
                missing_fixtures,
            });
        }

        self.resolve_root_fixtures(py, &regular_fixture_names)
    }

    pub(super) fn resolve_indirect_fixtures(
        &mut self,
        py: Python<'_>,
        test: &DiscoveredTestFunction,
        names: &HashSet<String>,
    ) -> FixtureResolutionResult<Vec<FixtureId>> {
        let mut names = names.iter().map(String::as_str).collect::<Vec<_>>();
        names.sort_unstable();
        let missing_fixtures = names
            .iter()
            .filter(|name| !self.lookup_root_fixture(name).is_found())
            .map(|name| (*name).to_owned())
            .collect::<Vec<_>>();
        if !missing_fixtures.is_empty() {
            return Err(FixtureResolutionError::MissingTestFixtures {
                definition: Rc::clone(test.definition()),
                missing_fixtures,
            });
        }
        self.resolve_root_fixtures(py, &names)
    }

    pub(super) fn fixture(&self, id: FixtureId) -> &NormalizedFixture {
        &self.fixtures[id.index()]
    }

    /// Resolves fixture roots after excluding names supplied by parametrization.
    pub(super) fn resolve_root_fixtures<N: AsRef<str>>(
        &mut self,
        py: Python,
        fixture_names: &[N],
    ) -> FixtureResolutionResult<Vec<FixtureId>> {
        let mut path = FixturePath::default();
        let mut fixtures = Vec::with_capacity(fixture_names.len());
        for name in fixture_names {
            let name = name.as_ref();
            if !self.request.names.direct.contains(name)
                && let FixtureLookup::Found(fixture) = self.lookup_root_fixture(name)
            {
                fixtures.push(self.normalize_fixture(py, fixture, &mut path)?);
            }
        }
        Ok(fixtures)
    }

    /// Only the selected definition can receive the case's scope override.
    fn has_indirect_case_for(&mut self, fixture: &DiscoveredFixture) -> bool {
        let name = fixture.name().function_name();
        self.request.names.indirect.contains(name)
            && matches!(
                self.lookup_root_fixture(name),
                FixtureLookup::Found(root) if root.identity() == fixture.identity()
            )
    }

    /// Resolves a list of names and validates every dependency scope edge.
    fn get_dependent_fixtures(
        &mut self,
        py: Python,
        current_fixture: &'a DiscoveredFixture,
        fixture_names: &[&str],
        path: &mut FixturePath<'a>,
    ) -> FixtureResolutionResult<Vec<FixtureDependency>> {
        let mut normalized_fixtures = Vec::with_capacity(fixture_names.len());
        let mut missing_fixtures = Vec::new();
        let mut rejected_fixtures = Vec::new();

        for &dep_name in fixture_names {
            if dep_name == "request" {
                continue;
            }
            if self.request.names.direct.contains(dep_name) {
                if !(current_fixture.scope().can_use(FixtureScope::Function)
                    || self.has_indirect_case_for(current_fixture))
                {
                    return Err(FixtureResolutionError::ParameterScopeMismatch {
                        fixture: FixtureResolutionEntry::new(current_fixture),
                        parameter: dep_name.to_owned(),
                    });
                }
                normalized_fixtures.push(FixtureDependency::Parameter(dep_name.into()));
                continue;
            }

            let lookup =
                lookup_fixture(Some(current_fixture), dep_name, self.parents, self.current);
            if current_fixture.name().function_name() == dep_name && !lookup.is_found() {
                let normalized = self.normalize_fixture(py, current_fixture, path)?;
                normalized_fixtures.push(FixtureDependency::Fixture(normalized));
                continue;
            }

            match lookup {
                FixtureLookup::Found(fixture) => {
                    if !current_fixture.scope().can_use(fixture.scope())
                        && !self.indirect_fixtures.contains(current_fixture.name())
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
                    normalized_fixtures.push(FixtureDependency::Fixture(normalized));
                }
                FixtureLookup::Rejected(rejected_fixture) => {
                    rejected_fixtures.push(rejected_fixture.clone());
                    missing_fixtures.push(dep_name.to_owned());
                }
                FixtureLookup::Missing => {
                    missing_fixtures.push(dep_name.to_owned());
                }
            }
        }

        if !missing_fixtures.is_empty() {
            return Err(FixtureResolutionError::MissingFixtures {
                fixture: FixtureResolutionEntry::new(current_fixture),
                missing_fixtures,
                rejected_fixtures,
            });
        }

        Ok(normalized_fixtures)
    }
}

pub(super) fn indirect_fixture_definition<'a>(
    parents: &'a [&'a DiscoveredPackage],
    current: &'a (dyn HasFixtures<'a> + 'a),
    name: &str,
) -> Option<&'a QualifiedFunctionName> {
    match lookup_fixture(None, name, parents, current) {
        FixtureLookup::Found(fixture) => Some(fixture.name()),
        _ => None,
    }
}

/// Looks up a fixture by name in the current node and parent packages.
/// Same-name dependencies skip the active provider and all providers nearer to
/// the test, so each override can reach its immediate ancestor without forming
/// a false cycle.
fn lookup_fixture<'a>(
    current_fixture: Option<&DiscoveredFixture>,
    name: &str,
    parents: &'a [&'a DiscoveredPackage],
    current: &'a (dyn HasFixtures<'a> + 'a),
) -> FixtureLookup<'a> {
    let parent_start = current_fixture
        .filter(|active| active.name().function_name() == name)
        .and_then(|active| {
            parents.iter().position(|parent| {
                matches!(
                    parent.lookup_fixture(active.name().function_name()),
                    FixtureLookup::Found(candidate) if std::ptr::eq(candidate, active)
                )
            })
        });

    if parent_start.is_none() {
        match current.lookup_fixture(name) {
            FixtureLookup::Found(fixture)
                if current_fixture.is_some_and(|current| std::ptr::eq(current, fixture)) => {}
            FixtureLookup::Missing => {}
            lookup => return lookup,
        }
    }

    let parents = parent_start.map_or(parents, |index| &parents[..index]);
    for parent in parents.iter().rev() {
        match parent.lookup_fixture(name) {
            FixtureLookup::Found(fixture)
                if current_fixture.is_some_and(|current| std::ptr::eq(current, fixture)) => {}
            FixtureLookup::Missing => {}
            lookup => return lookup,
        }
    }

    FixtureLookup::Missing
}
