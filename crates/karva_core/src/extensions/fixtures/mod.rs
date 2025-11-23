use pyo3::prelude::*;
use ruff_python_ast::{Expr, StmtFunctionDef};

mod builtins;
mod finalizer;
mod manager;
mod normalized_fixture;
pub mod python;
mod scope;
mod traits;
mod utils;

pub use builtins::get_builtin_fixture;
pub use finalizer::Finalizer;
pub use manager::{FixtureManager, get_auto_use_fixtures};
pub use normalized_fixture::{NormalizedFixture, NormalizedFixtureName, NormalizedFixtureValue};
pub use scope::FixtureScope;
pub use traits::{HasFixtures, RequiresFixtures};
pub use utils::missing_arguments_from_error;

use crate::{
    ModulePath, QualifiedFunctionName,
    extensions::fixtures::{scope::fixture_scope, utils::handle_custom_fixture_params},
};

#[derive(Clone)]
pub struct Fixture {
    name: QualifiedFunctionName,
    function_definition: StmtFunctionDef,
    scope: FixtureScope,
    auto_use: bool,
    function: Py<PyAny>,
    is_generator: bool,
    params: Option<Vec<Py<PyAny>>>,
}

impl Fixture {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        py: Python,
        name: QualifiedFunctionName,
        function_definition: StmtFunctionDef,
        scope: FixtureScope,
        auto_use: bool,
        function: Py<PyAny>,
        is_generator: bool,
        params: Option<Vec<Py<PyAny>>>,
    ) -> Self {
        Self {
            name,
            function_definition,
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

    pub(crate) const fn params(&self) -> Option<&Vec<Py<PyAny>>> {
        self.params.as_ref()
    }

    pub(crate) const fn function(&self) -> &Py<PyAny> {
        &self.function
    }

    pub(crate) const fn function_definition(&self) -> &StmtFunctionDef {
        &self.function_definition
    }

    pub(crate) fn try_from_function(
        py: Python<'_>,
        function_definition: &StmtFunctionDef,
        py_module: &Bound<'_, PyModule>,
        module_path: &ModulePath,
        is_generator_function: bool,
    ) -> Result<Option<Self>, String> {
        let function = py_module
            .getattr(function_definition.name.to_string())
            .map_err(|e| e.to_string())?;

        let try_karva = Self::try_from_karva_function(
            py,
            function_definition,
            &function,
            module_path.clone(),
            is_generator_function,
        );

        let try_karva_err = match try_karva {
            Ok(Some(fixture)) => return Ok(Some(fixture)),
            Ok(None) => None,
            Err(e) => Some(e),
        };

        let try_pytest = Self::try_from_pytest_function(
            py,
            function_definition,
            &function,
            module_path.clone(),
            is_generator_function,
        );

        match try_pytest {
            Ok(Some(fixture)) => Ok(Some(fixture)),
            Ok(None) => try_karva_err.map_or_else(|| Ok(None), Err),
            Err(e) => Err(e),
        }
    }

    pub(crate) fn try_from_pytest_function(
        py: Python<'_>,
        function_definition: &StmtFunctionDef,
        function: &Bound<'_, PyAny>,
        module_name: ModulePath,
        is_generator_function: bool,
    ) -> Result<Option<Self>, String> {
        let Some(found_name) =
            get_attribute(function.clone(), &["_fixture_function_marker", "name"])
        else {
            return Ok(None);
        };

        let Some(scope) = get_attribute(function.clone(), &["_fixture_function_marker", "scope"])
        else {
            return Ok(None);
        };

        let Some(auto_use) =
            get_attribute(function.clone(), &["_fixture_function_marker", "autouse"])
        else {
            return Ok(None);
        };

        let params = get_attribute(function.clone(), &["_fixture_function_marker", "params"])
            .and_then(|p| {
                if p.is_none() {
                    None
                } else {
                    p.extract::<Vec<Py<PyAny>>>().ok()
                }
            });

        let Some(function) = get_attribute(function.clone(), &["_fixture_function"]) else {
            return Ok(None);
        };

        let name = if found_name.is_none() {
            function_definition.name.to_string()
        } else {
            found_name.to_string()
        };

        let fixture_scope = fixture_scope(py, &scope, &name)?;

        Ok(Some(Self::new(
            py,
            QualifiedFunctionName::new(name, module_name),
            function_definition.clone(),
            fixture_scope,
            auto_use.extract::<bool>().unwrap_or(false),
            function.into(),
            is_generator_function,
            params,
        )))
    }

    pub(crate) fn try_from_karva_function(
        py: Python<'_>,
        function_def: &StmtFunctionDef,
        function: &Bound<'_, PyAny>,
        module_path: ModulePath,
        is_generator_function: bool,
    ) -> Result<Option<Self>, String> {
        let Ok(py_function) = function
            .clone()
            .cast_into::<python::FixtureFunctionDefinition>()
        else {
            return Ok(None);
        };

        let Ok(py_function_borrow) = py_function.try_borrow_mut() else {
            return Ok(None);
        };

        let scope_obj = py_function_borrow.scope.clone();
        let name = py_function_borrow.name.clone();
        let auto_use = py_function_borrow.auto_use;
        let params = py_function_borrow.params.clone();

        let fixture_scope = fixture_scope(py, scope_obj.bind(py), &name)?;

        Ok(Some(Self::new(
            py,
            QualifiedFunctionName::new(name, module_path),
            function_def.clone(),
            fixture_scope,
            auto_use,
            py_function.into(),
            is_generator_function,
            params,
        )))
    }
}

fn get_attribute<'a>(function: Bound<'a, PyAny>, attributes: &[&str]) -> Option<Bound<'a, PyAny>> {
    let mut current = function;
    for attribute in attributes {
        let current_attr = current.getattr(attribute).ok()?;
        current = current_attr;
    }
    Some(current.clone())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::fixtures::scope::resolve_dynamic_scope;

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
        Python::attach(|py| {
            let func = py.eval(c"lambda **kwargs: 'session'", None, None).unwrap();

            let resolved = resolve_dynamic_scope(py, &func, "test_fixture").unwrap();
            assert_eq!(resolved, FixtureScope::Session);
        });
    }

    #[test]
    fn test_resolve_dynamic_scope_with_fixture_name() {
        Python::attach(|py| {
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
        Python::attach(|py| {
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
        Python::attach(|py| {
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
