use std::fmt::Display;

use pyo3::{prelude::*, types::PyTuple};
use ruff_python_ast::{Decorator, Expr, StmtFunctionDef};

mod extractor;
mod finalizer;
mod manager;
pub mod python;

pub use finalizer::{Finalizer, Finalizers};
pub use manager::FixtureManager;

use crate::discovery::visitor::is_generator_function;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FixtureScope {
    #[default]
    Function,
    Module,
    Package,
    Session,
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

pub struct Fixture {
    name: String,
    function_def: StmtFunctionDef,
    scope: FixtureScope,
    auto_use: bool,
    function: Py<PyAny>,
    is_generator: bool,
}

impl Fixture {
    #[must_use]
    pub const fn new(
        name: String,
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
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn scope(&self) -> &FixtureScope {
        &self.scope
    }

    #[must_use]
    pub const fn is_generator(&self) -> bool {
        self.is_generator
    }

    #[must_use]
    pub const fn auto_use(&self) -> bool {
        self.auto_use
    }

    pub fn call<'a>(
        &self,
        py: Python<'a>,
        fixture_manager: &mut FixtureManager,
    ) -> PyResult<Bound<'a, PyAny>> {
        let mut required_fixtures = Vec::new();

        for name in self.get_required_fixture_names() {
            if let Some(fixture) = fixture_manager.get_fixture(&name) {
                required_fixtures.push(fixture.clone().into_bound(py));
            }
        }
        let args = PyTuple::new(py, required_fixtures)?;
        if self.is_generator {
            let function_return = self.function.call(py, args.clone(), None)?;

            let finalizer = Finalizer::new(self.name().to_string(), function_return.clone());
            fixture_manager.insert_finalizer(finalizer, self.scope());

            let self_return = function_return
                .call_method1(py, "__next__", args)
                .map(|r| r.into_bound(py))?;

            Ok(self_return)
        } else {
            let function_return = self.function.call(py, args, None);
            function_return.map(|r| r.into_bound(py))
        }
    }

    pub fn try_from_function(
        function_definition: &StmtFunctionDef,
        py_module: &Bound<'_, PyModule>,
    ) -> Result<Option<Self>, String> {
        let function = py_module
            .getattr(function_definition.name.to_string())
            .map_err(|e| e.to_string())?;

        let is_generator_function = is_generator_function(function_definition);

        let try_karva = extractor::try_from_karva_function(
            function_definition,
            &function,
            is_generator_function,
        );

        let try_karva_err = match try_karva {
            Ok(Some(fixture)) => return Ok(Some(fixture)),
            Ok(None) => None,
            Err(e) => Some(e),
        };

        let try_pytest = extractor::try_from_pytest_function(
            function_definition,
            &function,
            is_generator_function,
        );

        if let Some(fixture) = try_pytest {
            return Ok(Some(fixture));
        }

        if let Some(e) = try_karva_err {
            return Err(e);
        }

        Ok(None)
    }
}

impl HasFunctionDefinition for Fixture {
    fn function_definition(&self) -> &StmtFunctionDef {
        &self.function_def
    }
}

impl std::fmt::Debug for Fixture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Fixture(name: {}, scope: {})", self.name, self.scope)
    }
}

pub trait HasFunctionDefinition {
    #[must_use]
    fn get_required_fixture_names(&self) -> Vec<String> {
        let mut required_fixtures = Vec::new();
        for parameter in self
            .function_definition()
            .parameters
            .iter_non_variadic_params()
        {
            required_fixtures.push(parameter.parameter.name.as_str().to_string());
        }
        required_fixtures
    }

    fn function_definition(&self) -> &StmtFunctionDef;
}

pub trait RequiresFixtures: std::fmt::Debug {
    #[must_use]
    fn uses_fixture(&self, fixture_name: &str) -> bool {
        self.required_fixtures().contains(&fixture_name.to_string())
    }

    #[must_use]
    fn required_fixtures(&self) -> Vec<String>;
}

impl<T: HasFunctionDefinition + std::fmt::Debug> RequiresFixtures for T {
    fn required_fixtures(&self) -> Vec<String> {
        self.get_required_fixture_names()
    }
}

pub fn is_fixture_function(val: &StmtFunctionDef) -> bool {
    val.decorator_list.iter().any(is_fixture)
}

fn is_fixture(decorator: &Decorator) -> bool {
    match &decorator.expression {
        Expr::Name(name) => name.id == "fixture",
        Expr::Attribute(attr) => attr.attr.id == "fixture",
        Expr::Call(call) => match call.func.as_ref() {
            Expr::Name(name) => name.id == "fixture",
            Expr::Attribute(attr) => attr.attr.id == "fixture",
            _ => false,
        },
        _ => false,
    }
}

/// This trait is used to get all fixtures (from a module or package) that have a given scope.
///
/// For example, if we are in a test module, we want to get all fixtures used in the test module.
/// If we are in a package, we want to get all fixtures used in the package from the configuration module.
pub trait HasFixtures<'proj>: std::fmt::Debug {
    fn fixtures<'a: 'proj>(
        &'a self,
        scope: &[FixtureScope],
        test_cases: &[&dyn RequiresFixtures],
    ) -> Vec<&'proj Fixture> {
        let mut fixtures = Vec::new();
        for fixture in self.all_fixtures(test_cases) {
            if scope.contains(fixture.scope()) {
                fixtures.push(fixture);
            }
        }
        fixtures
    }

    fn get_fixture<'a: 'proj>(&'a self, fixture_name: &str) -> Option<&'proj Fixture> {
        self.all_fixtures(&[])
            .into_iter()
            .find(|fixture| fixture.name() == fixture_name)
    }

    fn all_fixtures<'a: 'proj>(
        &'a self,
        test_cases: &[&dyn RequiresFixtures],
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
}
