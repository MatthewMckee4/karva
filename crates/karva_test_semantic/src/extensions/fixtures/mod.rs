//! Fixture definitions, dependency resolution, caching, and teardown semantics.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use karva_python_semantic::{ModulePath, QualifiedFunctionName};
use pyo3::exceptions::PyAttributeError;
use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

mod finalizer;
mod normalized_fixture;
pub mod python;
mod scope;
mod traits;
mod utils;

pub use finalizer::Finalizer;
pub use normalized_fixture::{FixtureDependency, FixtureId, FixturePlan, NormalizedFixture};
pub use scope::FixtureScope;
pub use traits::{FixtureLookup, HasFixtures};
pub use utils::missing_arguments_from_error;

use crate::discovery::DiscoveredPackage;
use crate::discovery::models::definition::FunctionDefinition;
use crate::extensions::fixtures::python::InvalidFixtureError;
use crate::extensions::fixtures::scope::fixture_scope;

/// Cheap run-local key for one qualified fixture definition.
///
/// The qualified name is hashed once during discovery. Equality still compares
/// the full name, so independently discovered imports share scoped values and
/// hash collisions cannot merge distinct fixtures.
#[derive(Clone, Debug)]
pub struct FixtureIdentity {
    /// Source definition retained for diagnostics and execution.
    definition: Rc<FunctionDefinition>,

    /// Precomputed qualified-name hash used by fixture hot paths.
    hash: u64,
}

impl FixtureIdentity {
    fn new(definition: FunctionDefinition) -> Self {
        let definition = Rc::new(definition);
        let mut hasher = DefaultHasher::new();
        definition.name().hash(&mut hasher);
        Self {
            definition,
            hash: hasher.finish(),
        }
    }

    fn definition(&self) -> &Rc<FunctionDefinition> {
        &self.definition
    }

    fn name(&self) -> &QualifiedFunctionName {
        self.definition.name()
    }
}

impl PartialEq for FixtureIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name()
    }
}

impl Eq for FixtureIdentity {}

impl Hash for FixtureIdentity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash);
    }
}

/// Represents a pytest-style fixture discovered from Python source code.
///
/// Fixtures provide reusable setup and teardown logic for tests. They can be
/// scoped to function, module, package, or session level, and may optionally
/// be auto-used without explicit declaration.
#[derive(Clone, Debug)]
pub struct DiscoveredFixture {
    /// Qualified identity and immutable source definition.
    identity: FixtureIdentity,

    /// The scope at which this fixture's value is cached.
    scope: FixtureScope,

    /// Whether this fixture is automatically used without explicit request.
    auto_use: bool,

    /// Reference to the actual Python callable object. Wrapped in ``Rc`` so
    /// that ``DiscoveredFixture`` stays cheaply ``Clone`` without needing a
    /// Python token (``Py<T>`` only supports ``clone_ref(py)``).
    function: Rc<Py<PyAny>>,

    /// Whether this fixture is a generator (uses yield for teardown).
    is_generator: bool,
}

/// Fixture definition rejected during discovery.
#[derive(Clone, Debug)]
pub struct RejectedFixture {
    /// Public fixture name used for lookup and diagnostics.
    exposure_name: String,

    /// Discovery error retained for dependency diagnostics.
    reason: String,

    /// Immutable source identity and syntax.
    definition: Rc<FunctionDefinition>,
}

impl RejectedFixture {
    pub(crate) fn new(
        exposure_name: String,
        reason: String,
        stmt_function_def: Rc<StmtFunctionDef>,
        source_file: SourceFile,
        module_path: ModulePath,
    ) -> Self {
        let qualified_name =
            QualifiedFunctionName::new(stmt_function_def.name.to_string(), module_path);
        Self {
            exposure_name,
            reason,
            definition: Rc::new(FunctionDefinition::new(
                qualified_name,
                stmt_function_def,
                source_file,
            )),
        }
    }

    pub(crate) fn exposure_name(&self) -> &str {
        &self.exposure_name
    }

    pub(crate) fn reason(&self) -> &str {
        &self.reason
    }

    pub(crate) fn statement(&self) -> &StmtFunctionDef {
        self.definition.statement()
    }

    pub(crate) fn source_file(&self) -> &SourceFile {
        self.definition.source_file()
    }
}

impl DiscoveredFixture {
    fn new(
        name: QualifiedFunctionName,
        stmt_function_def: Rc<StmtFunctionDef>,
        source_file: SourceFile,
        scope: FixtureScope,
        auto_use: bool,
        function: Py<PyAny>,
        is_generator: bool,
    ) -> Self {
        Self {
            identity: FixtureIdentity::new(FunctionDefinition::new(
                name,
                stmt_function_def,
                source_file,
            )),
            scope,
            auto_use,
            function: Rc::new(function),
            is_generator,
        }
    }

    pub(crate) fn name(&self) -> &QualifiedFunctionName {
        self.identity.name()
    }

    pub(crate) fn identity(&self) -> &FixtureIdentity {
        &self.identity
    }

    pub(crate) fn definition(&self) -> &Rc<FunctionDefinition> {
        self.identity.definition()
    }

    pub(crate) fn scope(&self) -> FixtureScope {
        self.scope
    }

    pub(crate) fn is_generator(&self) -> bool {
        self.is_generator
    }

    fn auto_use(&self) -> bool {
        self.auto_use
    }

    pub(crate) fn function(&self) -> &Py<PyAny> {
        &self.function
    }

    pub(crate) fn stmt_function_def(&self) -> &Rc<StmtFunctionDef> {
        self.identity.definition().statement_rc()
    }

    /// Reads the fixture name exposed at runtime, falling back to its source symbol.
    pub(crate) fn exposure_name_from_function(
        stmt_function_def: &StmtFunctionDef,
        py_module: &Bound<'_, PyModule>,
    ) -> String {
        let fallback = stmt_function_def.name.as_str();
        let Ok(function) = py_module.getattr(fallback) else {
            return fallback.to_string();
        };

        Self::exposure_name_from_value(&function, fallback, None)
            .unwrap_or_else(|_| fallback.to_string())
    }

    /// An explicit decorator name takes precedence over the provider's Python binding.
    pub(crate) fn exposure_name_from_value(
        value: &Bound<'_, PyAny>,
        definition_name: &str,
        binding: Option<&str>,
    ) -> PyResult<String> {
        if let Ok(marker) = get_fixture_function_marker(value) {
            return pytest_fixture_name(&marker, binding.unwrap_or(definition_name));
        }

        let fixture = value
            .cast::<python::FixtureFunctionDefinition>()?
            .try_borrow()?;
        Ok(match binding {
            Some(name) if !fixture.has_explicit_name => name.to_owned(),
            _ => fixture.name.clone(),
        })
    }

    pub(crate) fn try_from_function(
        py: Python<'_>,
        stmt_function_def: Rc<StmtFunctionDef>,
        py_module: &Bound<'_, PyModule>,
        module_path: &ModulePath,
        source_file: SourceFile,
        is_generator_function: bool,
    ) -> PyResult<Self> {
        tracing::debug!("Trying to parse `{}` as a fixture", stmt_function_def.name);

        let function = py_module.getattr(stmt_function_def.name.as_str())?;
        let name =
            Self::exposure_name_from_value(&function, stmt_function_def.name.as_str(), None)?;

        Self::try_from_value(
            py,
            stmt_function_def,
            &function,
            module_path,
            source_file,
            is_generator_function,
            name,
        )
    }

    /// Gets the Python function whose definition supplies the fixture's signature.
    pub(crate) fn source_function<'py>(value: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        if let Ok(fixture) = value.cast::<python::FixtureFunctionDefinition>() {
            Ok(fixture.try_borrow()?.function.bind(value.py()).clone())
        } else {
            get_fixture_function(value)
        }
    }

    /// Builds a fixture from its runtime value and evaluates its scope once per registration.
    pub(crate) fn try_from_value(
        py: Python<'_>,
        stmt_function_def: Rc<StmtFunctionDef>,
        value: &Bound<'_, PyAny>,
        provider_path: &ModulePath,
        source_file: SourceFile,
        is_generator_function: bool,
        name: String,
    ) -> PyResult<Self> {
        let (scope, auto_use, function) = if let Ok(marker) = get_fixture_function_marker(value) {
            (
                marker.getattr("scope")?,
                marker.getattr("autouse")?.extract::<bool>()?,
                get_fixture_function(value)?,
            )
        } else {
            let fixture = value
                .cast::<python::FixtureFunctionDefinition>()?
                .try_borrow()?;
            (
                fixture.scope.bind(py).clone(),
                fixture.auto_use,
                value.clone(),
            )
        };

        let scope = fixture_scope(py, &scope, &name).map_err(InvalidFixtureError::new_err)?;
        Ok(Self::new(
            QualifiedFunctionName::new(name, provider_path.clone()),
            stmt_function_def,
            source_file,
            scope,
            auto_use,
            function.unbind(),
            is_generator_function,
        ))
    }
}

fn pytest_fixture_name(marker: &Bound<'_, PyAny>, fallback: &str) -> PyResult<String> {
    let name = marker.getattr("name")?;
    if name.is_none() {
        Ok(fallback.to_string())
    } else {
        name.extract()
    }
}

const MISSING_FIXTURE_INFO: &str = "Could not find fixture information";

/// Get the fixture function marker from a function.
///
/// The second name is for older versions of pytest.
fn get_fixture_function_marker<'py>(function: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    ["_fixture_function_marker", "_pytestfixturefunction"]
        .iter()
        .find_map(|name| function.getattr(*name).ok())
        .ok_or_else(|| PyAttributeError::new_err(MISSING_FIXTURE_INFO))
}

/// Get the fixture function from a function.
///
/// Falls back to the pre-8.0 pytest `__pytest_wrapped__.obj` path.
fn get_fixture_function<'py>(function: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    if let Ok(attr) = function.getattr("_fixture_function") {
        return Ok(attr);
    }

    if let Ok(wrapped) = function.getattr("__pytest_wrapped__")
        && let Ok(obj) = wrapped.getattr("obj")
    {
        return Ok(obj);
    }

    Err(PyAttributeError::new_err(MISSING_FIXTURE_INFO))
}

/// Resolves auto-use names through the visible fixture provider chain.
///
/// An inherited auto-use name also activates a nearer non-auto-use override;
/// rejected nearer definitions prevent the parent implementation from running.
pub fn get_auto_use_fixtures<'a>(
    parents: &'a [&'a DiscoveredPackage],
    current: &'a dyn HasFixtures<'a>,
    scope: FixtureScope,
) -> Vec<&'a DiscoveredFixture> {
    let current_fixtures = current.auto_use_fixtures(scope.scopes_above());
    let parent_fixtures = parents
        .iter()
        .rev()
        .flat_map(|parent| parent.auto_use_fixtures(scope.scopes_above()));

    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    current_fixtures
        .into_iter()
        .chain(parent_fixtures)
        .filter(|fixture| seen.insert(fixture.name().function_name()))
        .filter_map(|fixture| {
            let name = fixture.name().function_name();
            let lookup = std::iter::once(current.lookup_fixture(name))
                .chain(
                    parents
                        .iter()
                        .rev()
                        .map(|parent| parent.lookup_fixture(name)),
                )
                .find(|lookup| !matches!(lookup, FixtureLookup::Missing));
            match lookup {
                Some(FixtureLookup::Found(visible))
                    if scope.scopes_above().contains(&visible.scope()) =>
                {
                    Some(visible)
                }
                _ => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_fixture_scope() {
        assert_eq!(
            FixtureScope::try_from("invalid".to_string()),
            Err("Invalid fixture scope `invalid`".to_string())
        );
    }
}
