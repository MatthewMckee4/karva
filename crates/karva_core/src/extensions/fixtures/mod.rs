use std::sync::Arc;

use pyo3::{exceptions::PyAttributeError, prelude::*};
use ruff_python_ast::{Expr, StmtFunctionDef};

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
pub use python::FixtureRequest;
pub use scope::FixtureScope;
pub use traits::{HasFixtures, RequiresFixtures};
pub use utils::missing_arguments_from_error;

use crate::{
    ModulePath, QualifiedFunctionName,
    discovery::DiscoveredPackage,
    extensions::{
        fixtures::{
            python::InvalidFixtureError, scope::fixture_scope, utils::handle_custom_fixture_params,
        },
        tags::Parametrization,
    },
};

#[derive(Clone)]
pub struct Fixture {
    name: QualifiedFunctionName,
    stmt_function_def: Arc<StmtFunctionDef>,
    scope: FixtureScope,
    auto_use: bool,
    function: Py<PyAny>,
    is_generator: bool,
    params: Option<Vec<Parametrization>>,
}

impl Fixture {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        py: Python,
        name: QualifiedFunctionName,
        stmt_function_def: Arc<StmtFunctionDef>,
        scope: FixtureScope,
        auto_use: bool,
        function: Py<PyAny>,
        is_generator: bool,
        params: Option<Vec<Py<PyAny>>>,
    ) -> Self {
        Self {
            name,
            stmt_function_def,
            scope,
            auto_use,
            function,
            is_generator,
            params: params.map(|params| handle_custom_fixture_params(py, params)),
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

    pub(crate) const fn params(&self) -> Option<&Vec<Parametrization>> {
        self.params.as_ref()
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

        let params = fixture_function_marker
            .getattr("params")
            .ok()
            .and_then(|p| {
                if p.is_none() {
                    None
                } else {
                    p.extract::<Vec<Py<PyAny>>>().ok()
                }
            });

        let fixture_function = get_fixture_function(function)?;

        let name = if found_name.is_none() {
            stmt_function_def.name.to_string()
        } else {
            found_name.to_string()
        };

        let fixture_scope =
            fixture_scope(py, &scope, &name).map_err(InvalidFixtureError::new_err)?;

        Ok(Self::new(
            py,
            QualifiedFunctionName::new(name, module_name),
            stmt_function_def,
            fixture_scope,
            auto_use.extract::<bool>().unwrap_or(false),
            fixture_function.into(),
            is_generator_function,
            params,
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
        let params = py_function_borrow.params.clone();

        let fixture_scope =
            fixture_scope(py, scope_obj.bind(py), &name).map_err(InvalidFixtureError::new_err)?;

        Ok(Self::new(
            py,
            QualifiedFunctionName::new(name, module_path),
            stmt_function_def,
            fixture_scope,
            auto_use,
            py_function.into(),
            is_generator_function,
            params,
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

pub fn is_fixture_function(val: &StmtFunctionDef) -> bool {
    val.decorator_list
        .iter()
        .any(|decorator| is_fixture(&decorator.expression))
}

fn is_fixture(expr: &Expr) -> bool {
    match expr {
        Expr::Name(name) => name.id == "fixture",
        Expr::Attribute(attr) => attr.attr.id == "fixture",
        Expr::Call(call) => is_fixture(call.func.as_ref()),
        _ => false,
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
    use crate::{extensions::fixtures::scope::resolve_dynamic_scope, utils::attach};

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

    #[test]
    fn test_resolve_dynamic_scope() {
        attach(|py| {
            let func = py.eval(c"lambda **kwargs: 'session'", None, None).unwrap();

            let resolved = resolve_dynamic_scope(py, &func, "test_fixture").unwrap();
            assert_eq!(resolved, FixtureScope::Session);
        });
    }

    #[test]
    fn test_resolve_dynamic_scope_with_fixture_name() {
        attach(|py| {
            let func = py.eval(
                c"lambda **kwargs: 'session' if kwargs.get('fixture_name') == 'important_fixture' else 'function'",
                None,
                None
            ).unwrap();

            let resolved_important = resolve_dynamic_scope(py, &func, "important_fixture").unwrap();
            assert_eq!(resolved_important, FixtureScope::Session);

            let resolved_normal = resolve_dynamic_scope(py, &func, "normal_fixture").unwrap();
            assert_eq!(resolved_normal, FixtureScope::Function);
        });
    }

    #[test]
    fn test_resolve_dynamic_scope_invalid_return() {
        attach(|py| {
            let func = py
                .eval(c"lambda **kwargs: 'invalid_scope'", None, None)
                .unwrap();

            let result = resolve_dynamic_scope(py, &func, "test_fixture");
            assert!(result.is_err());
            assert!(result.unwrap_err().contains("Invalid fixture scope"));
        });
    }

    #[test]
    fn test_resolve_dynamic_scope_exception() {
        attach(|py| {
            let func = py.eval(c"lambda **kwargs: 1/0", None, None).unwrap();

            let result = resolve_dynamic_scope(py, &func, "test_fixture");
            assert!(result.is_err());
            assert!(
                result
                    .unwrap_err()
                    .contains("Failed to call dynamic scope function")
            );
        });
    }
}
