use std::fmt::Display;

use pyo3::{prelude::*, types::PyTuple};
use ruff_python_ast::{Expr, StmtFunctionDef};

pub mod extractor;
pub mod finalizer;
pub mod manager;

pub mod python;

pub(crate) use finalizer::{Finalizer, Finalizers};
pub(crate) use manager::FixtureManager;

use crate::name::QualifiedFunctionName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FixtureScope {
    Function,
    Module,
    Package,
    Session,
}

impl Default for FixtureScope {
    fn default() -> Self {
        Self::Function
    }
}

impl PartialOrd for FixtureScope {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        const fn rank(scope: &FixtureScope) -> usize {
            match scope {
                FixtureScope::Function => 0,
                FixtureScope::Module => 1,
                FixtureScope::Package => 2,
                FixtureScope::Session => 3,
            }
        }
        let self_rank = rank(self);
        let other_rank = rank(other);
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
    function_def: StmtFunctionDef,
    scope: FixtureScope,
    auto_use: bool,
    function: Py<PyAny>,
    is_generator: bool,
}

impl Fixture {
    #[must_use]
    pub(crate) const fn new(
        name: QualifiedFunctionName,
        function_def: StmtFunctionDef,
        scope: FixtureScope,
        auto_use: bool,
        function: Py<PyAny>,
        is_generator: bool,
    ) -> Self {
        Self {
            name,
            function_def,
            scope,
            auto_use,
            function,
            is_generator,
        }
    }

    #[must_use]
    pub(crate) const fn name(&self) -> &QualifiedFunctionName {
        &self.name
    }

    #[must_use]
    pub(crate) const fn scope(&self) -> &FixtureScope {
        &self.scope
    }

    #[must_use]
    pub(crate) const fn is_generator(&self) -> bool {
        self.is_generator
    }

    #[must_use]
    pub(crate) const fn auto_use(&self) -> bool {
        self.auto_use
    }

    pub(crate) fn call<'a>(
        &self,
        py: Python<'a>,
        fixture_manager: &mut FixtureManager,
    ) -> PyResult<Bound<'a, PyAny>> {
        let mut required_fixtures = Vec::new();

        for name in self.dependant_fixtures(py) {
            if let Some(fixture) =
                fixture_manager.get_fixture_with_name(&name, Some(&[self.name()]))
            {
                required_fixtures.push(fixture.clone().into_bound(py));
            }
        }
        let args = PyTuple::new(py, required_fixtures)?;

        if self.is_generator() {
            let mut generator = self
                .function
                .bind(py)
                .call(args.clone(), None)?
                .downcast_into()?;

            let finalizer = Finalizer::new(self.name().to_string(), generator.clone().unbind());
            fixture_manager.insert_finalizer(finalizer, self.scope());

            generator
                .next()
                .expect("generator should yield at least once")
        } else {
            let function_return = self.function.call(py, args, None);
            function_return.map(|r| r.into_bound(py))
        }
    }

    pub(crate) fn try_from_function(
        py: Python<'_>,
        function_definition: &StmtFunctionDef,
        py_module: &Bound<'_, PyModule>,
        module_name: &str,
        is_generator_function: bool,
    ) -> Result<Option<Self>, String> {
        let function = py_module
            .getattr(function_definition.name.to_string())
            .map_err(|e| e.to_string())?;

        let try_karva = extractor::try_from_karva_function(
            py,
            function_definition,
            &function,
            module_name,
            is_generator_function,
        );

        let try_karva_err = match try_karva {
            Ok(Some(fixture)) => return Ok(Some(fixture)),
            Ok(None) => None,
            Err(e) => Some(e),
        };

        let try_pytest = extractor::try_from_pytest_function(
            py,
            function_definition,
            &function,
            module_name,
            is_generator_function,
        );

        match try_pytest {
            Ok(Some(fixture)) => Ok(Some(fixture)),
            Ok(None) => try_karva_err.map_or_else(|| Ok(None), Err),
            Err(e) => Err(e),
        }
    }
}

impl std::fmt::Debug for Fixture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fixture(name: {}, scope: {})", self.name(), self.scope)
    }
}

/// This trait is used to represent an object that may require fixtures to be called before it is run.
pub(crate) trait UsesFixtures: std::fmt::Debug {
    #[must_use]
    fn uses_fixture(&self, py: Python<'_>, fixture_name: &str) -> bool {
        self.dependant_fixtures(py)
            .contains(&fixture_name.to_string())
    }

    #[must_use]
    fn dependant_fixtures(&self, py: Python<'_>) -> Vec<String>;
}

impl UsesFixtures for StmtFunctionDef {
    fn dependant_fixtures(&self, _py: Python<'_>) -> Vec<String> {
        let mut required_fixtures = Vec::new();
        for parameter in self.parameters.iter_non_variadic_params() {
            required_fixtures.push(parameter.parameter.name.as_str().to_string());
        }
        required_fixtures
    }
}

impl UsesFixtures for Fixture {
    fn dependant_fixtures(&self, py: Python<'_>) -> Vec<String> {
        self.function_def.dependant_fixtures(py)
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

/// This trait is used to get all fixtures (from a module or package) that have a given scope.
///
/// For example, if we are in a test module, we want to get all fixtures used in the test module.
/// If we are in a package, we want to get all fixtures used in the package from the configuration module.
pub(crate) trait HasFixtures<'proj>: std::fmt::Debug {
    fn fixtures<'a: 'proj>(
        &'a self,
        py: Python<'_>,
        scope: &[FixtureScope],
        test_cases: &[&dyn UsesFixtures],
    ) -> Vec<&'proj Fixture> {
        let mut fixtures = Vec::new();
        for fixture in self.all_fixtures(py, test_cases) {
            if scope.contains(fixture.scope()) {
                fixtures.push(fixture);
            }
        }
        fixtures
    }

    fn get_fixture<'a: 'proj>(
        &'a self,
        py: Python<'_>,
        fixture_name: &str,
    ) -> Option<&'proj Fixture> {
        self.all_fixtures(py, &[])
            .into_iter()
            .find(|fixture| fixture.name().function_name() == fixture_name)
    }

    fn all_fixtures<'a: 'proj>(
        &'a self,
        py: Python<'_>,
        test_cases: &[&dyn UsesFixtures],
    ) -> Vec<&'proj Fixture>;
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
        Python::with_gil(|py| {
            let func = py.eval(c"lambda **kwargs: 'session'", None, None).unwrap();

            let resolved = resolve_dynamic_scope(py, &func, "test_fixture").unwrap();
            assert_eq!(resolved, FixtureScope::Session);
        });
    }

    #[test]
    fn test_resolve_dynamic_scope_with_fixture_name() {
        Python::with_gil(|py| {
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
        Python::with_gil(|py| {
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
        Python::with_gil(|py| {
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
