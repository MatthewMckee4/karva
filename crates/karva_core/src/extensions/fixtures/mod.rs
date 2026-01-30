use std::sync::Arc;

use karva_python_semantic::{ModulePath, QualifiedFunctionName};
use pyo3::exceptions::PyAttributeError;
use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

mod builtins;
mod finalizer;
mod normalized_fixture;
pub mod python;
mod scope;
mod traits;
mod utils;

pub use builtins::{MockEnv, create_fixture_with_finalizer, get_builtin_fixture};
pub use finalizer::Finalizer;
pub use normalized_fixture::{NormalizedFixture, UserDefinedFixture};
pub use scope::FixtureScope;
pub use traits::{HasFixtures, RequiresFixtures};
pub use utils::missing_arguments_from_error;

use crate::discovery::DiscoveredPackage;
use crate::extensions::fixtures::python::InvalidFixtureError;
use crate::extensions::fixtures::scope::fixture_scope;

#[derive(Clone)]
pub struct Fixture {
    name: QualifiedFunctionName,
    stmt_function_def: Arc<StmtFunctionDef>,
    scope: FixtureScope,
    auto_use: bool,
    function: Py<PyAny>,
    is_generator: bool,
}

impl Fixture {
    pub(crate) fn new(
        name: QualifiedFunctionName,
        stmt_function_def: Arc<StmtFunctionDef>,
        scope: FixtureScope,
        auto_use: bool,
        function: Py<PyAny>,
        is_generator: bool,
    ) -> Self {
        Self {
            name,
            stmt_function_def,
            scope,
            auto_use,
            function,
            is_generator,
        }
    }

    pub(crate) const fn name(&self) -> &QualifiedFunctionName {
        &self.name
    }

    pub(crate) const fn scope(&self) -> FixtureScope {
        self.scope
    }

    pub(crate) const fn is_generator(&self) -> bool {
        self.is_generator
    }

    pub(crate) const fn auto_use(&self) -> bool {
        self.auto_use
    }

    pub(crate) const fn function(&self) -> &Py<PyAny> {
        &self.function
    }

    pub(crate) const fn stmt_function_def(&self) -> &Arc<StmtFunctionDef> {
        &self.stmt_function_def
    }

    pub(crate) fn try_from_function(
        py: Python<'_>,
        stmt_function_def: Arc<StmtFunctionDef>,
        py_module: &Bound<'_, PyModule>,
        module_path: &ModulePath,
        is_generator_function: bool,
    ) -> PyResult<Self> {
        tracing::debug!("Trying to parse `{}` as a fixture", stmt_function_def.name);

        let function = py_module.getattr(stmt_function_def.name.to_string())?;

        let try_karva = Self::try_from_karva_function(
            py,
            stmt_function_def.clone(),
            &function,
            module_path.clone(),
            is_generator_function,
        );

        let try_karva_err = match try_karva {
            Ok(fixture) => return Ok(fixture),
            Err(e) => {
                tracing::debug!("Failed to create fixture from Karva function: {}", e);
                Some(e)
            }
        };

        let try_pytest = Self::try_from_pytest_function(
            py,
            stmt_function_def,
            &function,
            module_path.clone(),
            is_generator_function,
        );

        match try_pytest {
            Ok(fixture) => Ok(fixture),
            Err(e) => {
                tracing::debug!("Failed to create fixture from Pytest function: {}", e);
                Err(try_karva_err.unwrap_or(e))
            }
        }
    }

    pub(crate) fn try_from_pytest_function(
        py: Python<'_>,
        stmt_function_def: Arc<StmtFunctionDef>,
        function: &Bound<'_, PyAny>,
        module_name: ModulePath,
        is_generator_function: bool,
    ) -> PyResult<Self> {
        let fixture_function_marker = get_fixture_function_marker(function)?;

        let found_name = fixture_function_marker.getattr("name")?;

        let scope = fixture_function_marker.getattr("scope")?;

        let auto_use = fixture_function_marker.getattr("autouse")?;

        let fixture_function = get_fixture_function(function)?;

        let name = if found_name.is_none() {
            stmt_function_def.name.to_string()
        } else {
            found_name.to_string()
        };

        let fixture_scope =
            fixture_scope(py, &scope, &name).map_err(InvalidFixtureError::new_err)?;

        Ok(Self::new(
            QualifiedFunctionName::new(name, module_name),
            stmt_function_def,
            fixture_scope,
            auto_use.extract::<bool>().unwrap_or(false),
            fixture_function.into(),
            is_generator_function,
        ))
    }

    pub(crate) fn try_from_karva_function(
        py: Python<'_>,
        stmt_function_def: Arc<StmtFunctionDef>,
        function: &Bound<'_, PyAny>,
        module_path: ModulePath,
        is_generator_function: bool,
    ) -> PyResult<Self> {
        let py_function = function
            .clone()
            .cast_into::<python::FixtureFunctionDefinition>()?;

        let py_function_borrow = py_function.try_borrow_mut()?;

        let scope_obj = py_function_borrow.scope.clone();
        let name = py_function_borrow.name.clone();
        let auto_use = py_function_borrow.auto_use;

        let fixture_scope =
            fixture_scope(py, scope_obj.bind(py), &name).map_err(InvalidFixtureError::new_err)?;

        Ok(Self::new(
            QualifiedFunctionName::new(name, module_path),
            stmt_function_def,
            fixture_scope,
            auto_use,
            py_function.into(),
            is_generator_function,
        ))
    }
}

/// Get the fixture function marker from a function.
fn get_fixture_function_marker<'py>(function: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let attribute_names = ["_fixture_function_marker", "_pytestfixturefunction"];

    // Older versions of pytest
    for name in attribute_names {
        if let Ok(attr) = function.getattr(name) {
            return Ok(attr);
        }
    }

    Err(PyAttributeError::new_err(
        "Could not find fixture information",
    ))
}

/// Get the fixture function from a function.
fn get_fixture_function<'py>(function: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    if let Ok(attr) = function.getattr("_fixture_function") {
        return Ok(attr);
    }

    // Older versions of pytest
    if let Ok(attr) = function.getattr("__pytest_wrapped__") {
        if let Ok(attr) = attr.getattr("obj") {
            return Ok(attr);
        }
    }

    Err(PyAttributeError::new_err(
        "Could not find fixture information",
    ))
}

impl std::fmt::Debug for Fixture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Fixture(name: {}, scope: {}, auto_use: {})",
            self.name(),
            self.scope(),
            self.auto_use()
        )
    }
}

pub fn get_auto_use_fixtures<'a>(
    parents: &'a [&'a DiscoveredPackage],
    current: &'a dyn HasFixtures<'a>,
    scope: FixtureScope,
) -> Vec<&'a Fixture> {
    let mut auto_use_fixtures_called = Vec::new();
    let auto_use_fixtures = current.auto_use_fixtures(&scope.scopes_above());

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

    for parent in parents {
        let parent_fixtures = parent.auto_use_fixtures(&[scope]);
        for fixture in parent_fixtures {
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
    }

    auto_use_fixtures_called
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_fixture_scope() {
        assert_eq!(
            FixtureScope::try_from("invalid".to_string()),
            Err("Invalid fixture scope: invalid".to_string())
        );
    }

    #[test]
    fn test_fixture_scope_display() {
        assert_eq!(FixtureScope::Function.to_string(), "function");
        assert_eq!(FixtureScope::Module.to_string(), "module");
        assert_eq!(FixtureScope::Package.to_string(), "package");
        assert_eq!(FixtureScope::Session.to_string(), "session");
    }
}
