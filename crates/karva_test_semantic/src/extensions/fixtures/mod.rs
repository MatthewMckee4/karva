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
pub use normalized_fixture::{FixtureId, FixturePlan, NormalizedFixture};
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

        if let Ok(marker) = get_fixture_function_marker(&function)
            && let Ok(name) = pytest_fixture_name(&marker, fallback)
        {
            return name;
        }

        if let Ok(function) = function.cast::<python::FixtureFunctionDefinition>()
            && let Ok(function) = function.try_borrow()
        {
            return function.name.clone();
        }

        fallback.to_string()
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

        let function = py_module.getattr(stmt_function_def.name.to_string())?;

        if get_fixture_function_marker(&function).is_ok() {
            return Self::try_from_pytest_function(
                py,
                stmt_function_def,
                &function,
                module_path.clone(),
                source_file,
                is_generator_function,
            );
        }

        Self::try_from_karva_function(
            py,
            stmt_function_def,
            &function,
            module_path.clone(),
            source_file,
            is_generator_function,
        )
    }

    fn try_from_pytest_function(
        py: Python<'_>,
        stmt_function_def: Rc<StmtFunctionDef>,
        function: &Bound<'_, PyAny>,
        module_name: ModulePath,
        source_file: SourceFile,
        is_generator_function: bool,
    ) -> PyResult<Self> {
        let fixture_function_marker = get_fixture_function_marker(function)?;

        let scope = fixture_function_marker.getattr("scope")?;

        let auto_use = fixture_function_marker.getattr("autouse")?;

        let fixture_function = get_fixture_function(function)?;

        let name = pytest_fixture_name(&fixture_function_marker, stmt_function_def.name.as_str())?;

        let fixture_scope =
            fixture_scope(py, &scope, &name).map_err(InvalidFixtureError::new_err)?;

        Ok(Self::new(
            QualifiedFunctionName::new(name, module_name),
            stmt_function_def,
            source_file,
            fixture_scope,
            auto_use.extract::<bool>()?,
            fixture_function.into(),
            is_generator_function,
        ))
    }

    fn try_from_karva_function(
        py: Python<'_>,
        stmt_function_def: Rc<StmtFunctionDef>,
        function: &Bound<'_, PyAny>,
        module_path: ModulePath,
        source_file: SourceFile,
        is_generator_function: bool,
    ) -> PyResult<Self> {
        let py_function = function
            .clone()
            .cast_into::<python::FixtureFunctionDefinition>()?;

        let py_function_borrow = py_function.try_borrow_mut()?;

        let scope_obj = py_function_borrow.scope.clone_ref(py);
        let name = py_function_borrow.name.clone();
        let auto_use = py_function_borrow.auto_use;

        let fixture_scope =
            fixture_scope(py, scope_obj.bind(py), &name).map_err(InvalidFixtureError::new_err)?;

        Ok(Self::new(
            QualifiedFunctionName::new(name, module_path),
            stmt_function_def,
            source_file,
            fixture_scope,
            auto_use,
            py_function.into(),
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

/// Resolves visible auto-use fixtures with nearer definitions shadowing parent names.
pub fn get_auto_use_fixtures<'a>(
    parents: &'a [&'a DiscoveredPackage],
    current: &'a dyn HasFixtures<'a>,
    scope: FixtureScope,
) -> Vec<&'a DiscoveredFixture> {
    let current_fixtures = current.auto_use_fixtures(scope.scopes_above());
    let parent_fixtures = parents
        .iter()
        .rev()
        .flat_map(|parent| parent.auto_use_fixtures(&[scope]));

    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    current_fixtures
        .into_iter()
        .chain(parent_fixtures)
        .filter(|fixture| seen.insert(fixture.name().function_name()))
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
