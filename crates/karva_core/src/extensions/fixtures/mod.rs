use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
};

use pyo3::{prelude::*, types::PyDict};
use ruff_python_ast::{Expr, StmtFunctionDef};

pub mod builtins;
pub mod finalizer;
pub mod manager;
pub mod python;
mod traits;
pub mod utils;

pub(crate) use finalizer::{Finalizer, Finalizers};
pub(crate) use manager::FixtureManager;
pub(crate) use traits::{HasFixtures, RequiresFixtures};
pub(crate) use utils::{handle_missing_fixtures, missing_arguments_from_error};

use crate::{
    diagnostic::{Diagnostic, FunctionDefinitionLocation, FunctionKind},
    discovery::DiscoveredModule,
    extensions::fixtures::python::FixtureRequest,
    name::{ModulePath, QualifiedFunctionName},
    utils::{cartesian_insert, function_definition_location},
};

#[derive(Copy, Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum FixtureScope {
    #[default]
    Function,
    Module,
    Package,
    Session,
}

impl FixtureScope {
    pub(crate) fn scopes_above(self) -> Vec<Self> {
        use FixtureScope::{Function, Module, Package, Session};

        match self {
            Function => vec![Function, Module, Package, Session],
            Module => vec![Module, Package, Session],
            Package => vec![Package, Session],
            Session => vec![Session],
        }
    }
}

impl PartialOrd for FixtureScope {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        const fn rank(scope: FixtureScope) -> usize {
            match scope {
                FixtureScope::Function => 0,
                FixtureScope::Module => 1,
                FixtureScope::Package => 2,
                FixtureScope::Session => 3,
            }
        }
        let self_rank = rank(*self);
        let other_rank = rank(*other);
        Some(self_rank.cmp(&other_rank))
    }
}

impl TryFrom<String> for FixtureScope {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.as_str() {
            "module" => Ok(Self::Module),
            "session" => Ok(Self::Session),
            "package" => Ok(Self::Package),
            "function" => Ok(Self::Function),
            _ => Err(format!("Invalid fixture scope: {s}")),
        }
    }
}

impl Display for FixtureScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Module => write!(f, "module"),
            Self::Session => write!(f, "session"),
            Self::Package => write!(f, "package"),
            Self::Function => write!(f, "function"),
        }
    }
}

/// Resolve a dynamic scope function to a concrete `FixtureScope`
pub(crate) fn resolve_dynamic_scope(
    py: Python<'_>,
    scope_fn: &Bound<'_, PyAny>,
    fixture_name: &str,
) -> Result<FixtureScope, String> {
    let kwargs = pyo3::types::PyDict::new(py);
    kwargs
        .set_item("fixture_name", fixture_name)
        .map_err(|e| format!("Failed to set fixture_name: {e}"))?;

    // TODO: Support config
    kwargs
        .set_item("config", py.None())
        .map_err(|e| format!("Failed to set config: {e}"))?;

    let result = scope_fn
        .call((), Some(&kwargs))
        .map_err(|e| format!("Failed to call dynamic scope function: {e}"))?;

    let scope_str = result
        .extract::<String>()
        .map_err(|e| format!("Dynamic scope function must return a string: {e}"))?;

    FixtureScope::try_from(scope_str)
}

pub(crate) struct Fixture {
    name: QualifiedFunctionName,
    function_definition: StmtFunctionDef,
    scope: FixtureScope,
    auto_use: bool,
    function: Py<PyAny>,
    is_generator: bool,
    params: Option<Vec<Py<PyAny>>>,
}

impl Fixture {
    pub(crate) const fn new(
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
            params,
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

    pub(crate) fn call<'a>(
        &self,
        py: Python<'a>,
        fixture_manager: &mut FixtureManager,
        module: &DiscoveredModule,
    ) -> Result<Vec<Bound<'a, PyAny>>, Diagnostic> {
        // A hashmap of fixtures for each param
        let mut each_call_fixtures: Vec<HashMap<String, Py<PyAny>>> = vec![HashMap::new()];

        let param_names = self.required_fixtures(py);

        let mut missing_fixtures = HashSet::new();

        for name in &param_names {
            if name == "request" {
                let params = match &self.params {
                    Some(p) if !p.is_empty() => p,
                    _ => &vec![py.None()],
                };

                each_call_fixtures = cartesian_insert(each_call_fixtures, params, name, |param| {
                    let param_value = param.clone();
                    let request = FixtureRequest::new(param_value);
                    let request_obj = Py::new(py, request)?;
                    Ok(request_obj.into_any())
                })
                .unwrap();
            } else if let Some(fixture_returns) =
                fixture_manager.get_fixture_with_name(py, name, Some(&[self.name()]))
            {
                each_call_fixtures = cartesian_insert(
                    each_call_fixtures,
                    &fixture_returns,
                    name,
                    |fixture_return| Ok(fixture_return.clone()),
                )
                .unwrap();
            } else {
                missing_fixtures.insert(name.clone());
            }
        }

        let test_case_location = function_definition_location(module, &self.function_definition);

        if !missing_fixtures.is_empty() {
            return Err(Diagnostic::missing_fixtures(
                missing_fixtures.into_iter().collect(),
                test_case_location,
                self.name().to_string(),
                FunctionKind::Fixture,
            ));
        }

        let mut res = Vec::new();

        for fixtures in each_call_fixtures {
            let kwargs = PyDict::new(py);

            for (key, value) in fixtures {
                let _ = kwargs.set_item(key, value);
            }

            let default_diagnostic = |err: PyErr| {
                Diagnostic::from_fixture_fail(
                    py,
                    &err,
                    FunctionDefinitionLocation::new(
                        self.name().to_string(),
                        test_case_location.clone(),
                    ),
                )
            };

            res.push(if self.is_generator() {
                match self.function.bind(py).call((), Some(&kwargs)) {
                    Ok(generator) => {
                        let mut generator = generator.cast_into().unwrap();

                        let finalizer =
                            Finalizer::new(self.name().to_string(), generator.clone().unbind());
                        fixture_manager.insert_finalizer(finalizer, self.scope());

                        generator
                            .next()
                            .expect("generator should yield at least once")
                            .unwrap()
                    }
                    Err(err) => return Err(default_diagnostic(err)),
                }
            } else {
                let function_return = self.function.call(py, (), Some(&kwargs));
                match function_return.map(|r| r.into_bound(py)) {
                    Ok(return_value) => return_value,
                    Err(err) => return Err(default_diagnostic(err)),
                }
            });
        }

        Ok(res)
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

fn fixture_scope(
    py: Python<'_>,
    scope_obj: &Bound<'_, PyAny>,
    name: &str,
) -> Result<FixtureScope, String> {
    if scope_obj.is_callable() {
        resolve_dynamic_scope(py, scope_obj, name)
    } else if let Ok(scope_str) = scope_obj.extract::<String>() {
        FixtureScope::try_from(scope_str)
    } else {
        Err("Scope must be either a string or a callable".to_string())
    }
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

pub(crate) fn is_fixture_function(val: &StmtFunctionDef) -> bool {
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
