use std::{
    collections::HashMap,
    fmt::Display,
    hash::{Hash, Hasher},
};

use karva_project::{path::SystemPathBuf, utils::module_name};
use pyo3::prelude::*;
use ruff_python_ast::{Decorator, Expr, StmtFunctionDef};

use crate::{
    case::TestCase, fixture::python::FixtureFunctionDefinition, utils::recursive_add_to_sys_path,
};

pub mod python;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FixtureScope {
    #[default]
    Function,
    Module,
    Package,
    Session,
}

impl From<&str> for FixtureScope {
    fn from(s: &str) -> Self {
        match s {
            "module" => Self::Module,
            "session" => Self::Session,
            "package" => Self::Package,
            _ => Self::Function,
        }
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
        write!(f, "{self:?}")
    }
}

#[must_use]
pub fn check_valid_scope(scope: &str) -> bool {
    matches!(scope, "module" | "session" | "function" | "package")
}

#[derive(Debug)]
pub struct Fixture {
    name: String,
    function_def: StmtFunctionDef,
    scope: FixtureScope,
    function: Py<FixtureFunctionDefinition>,
}

impl Fixture {
    #[must_use]
    pub const fn new(
        name: String,
        function_def: StmtFunctionDef,
        scope: FixtureScope,
        function: Py<FixtureFunctionDefinition>,
    ) -> Self {
        Self {
            name,
            function_def,
            scope,
            function,
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn function_def(&self) -> &StmtFunctionDef {
        &self.function_def
    }

    #[must_use]
    pub const fn scope(&self) -> &FixtureScope {
        &self.scope
    }

    pub fn from(
        py: &Python<'_>,
        val: StmtFunctionDef,
        path: &SystemPathBuf,
        cwd: &SystemPathBuf,
    ) -> Result<Self, String> {
        recursive_add_to_sys_path(py, path, cwd).map_err(|e| e.to_string())?;

        let module = module_name(cwd, path);

        let function = py
            .import(module)
            .map_err(|e| e.to_string())?
            .getattr(val.name.to_string())
            .map_err(|e| e.to_string())?;

        let py_function = function
            .downcast_into::<FixtureFunctionDefinition>()
            .map_err(|e| e.to_string())?;

        let scope = py_function.borrow_mut().scope.clone();
        let name = py_function.borrow_mut().name.clone();

        Ok(Self::new(
            name,
            val,
            FixtureScope::try_from(scope)?,
            py_function.into(),
        ))
    }

    pub fn call<'a>(&self, py: Python<'a>) -> PyResult<Bound<'a, PyAny>> {
        self.function.call(py, (), None).map(|r| r.into_bound(py))
    }
}

impl Hash for Fixture {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

impl PartialEq for Fixture {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

impl Eq for Fixture {}

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

pub type CalledFixtures<'a> = HashMap<String, Bound<'a, PyAny>>;

#[must_use]
pub fn call_fixtures<'a>(fixtures: &[&Fixture], py: Python<'a>) -> CalledFixtures<'a> {
    fixtures
        .iter()
        .filter_map(|fixture| match fixture.call(py) {
            Ok(fixture_return) => Some((fixture.name().to_string(), fixture_return)),
            Err(e) => {
                tracing::error!("Failed to call fixture {}: {}", fixture.name, e);
                None
            }
        })
        .collect()
}

pub trait HasFixtures<'proj> {
    fn fixtures<'a: 'proj>(
        &'a self,
        scope: &[FixtureScope],
        test_cases: Option<&[&TestCase]>,
    ) -> Vec<&'proj Fixture> {
        self.all_fixtures(test_cases)
            .into_iter()
            .filter(|fixture| scope.contains(fixture.scope()))
            .collect()
    }

    fn called_fixtures<'a: 'proj>(
        &'a self,
        py: Python<'proj>,
        scope: &[FixtureScope],
        test_cases: &[&TestCase],
    ) -> CalledFixtures<'proj> {
        call_fixtures(&self.fixtures(scope, Some(test_cases)), py)
    }

    fn all_fixtures<'a: 'proj>(&'a self, test_cases: Option<&[&TestCase]>) -> Vec<&'proj Fixture>;
}

#[derive(Debug)]
pub struct TestCaseFixtures<'a> {
    fixtures: &'a CalledFixtures<'a>,
}

impl<'a> TestCaseFixtures<'a> {
    #[must_use]
    pub const fn new(fixtures: &'a CalledFixtures<'a>) -> Self {
        Self { fixtures }
    }

    #[must_use]
    pub fn get_fixture(&self, fixture_name: &str) -> Option<&Bound<'a, PyAny>> {
        self.fixtures.get(fixture_name)
    }
}
