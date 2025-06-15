use std::{
    collections::HashMap,
    fmt::Display,
    hash::{Hash, Hasher},
};

use pyo3::{prelude::*, types::PyTuple};
use ruff_python_ast::{Decorator, Expr, StmtFunctionDef};

use crate::case::TestCase;

mod extractor;
mod manager;
pub mod python;

pub use extractor::FixtureExtractor;
pub use manager::FixtureManager;

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
    function: Py<PyAny>,
}

impl Fixture {
    #[must_use]
    pub const fn new(
        name: String,
        function_def: StmtFunctionDef,
        scope: FixtureScope,
        function: Py<PyAny>,
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
    pub const fn scope(&self) -> &FixtureScope {
        &self.scope
    }

    pub fn call<'a>(
        &self,
        py: Python<'a>,
        required_fixtures: Vec<Bound<'a, PyAny>>,
    ) -> PyResult<Bound<'a, PyAny>> {
        let args = PyTuple::new(py, required_fixtures)?;
        let function_return = self.function.call(py, args, None);
        function_return.map(|r| r.into_bound(py))
    }
}

impl FixtureRequester for Fixture {
    fn function_definition(&self) -> &StmtFunctionDef {
        &self.function_def
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

pub trait FixtureRequester {
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

    fn get_fixture<'a: 'proj>(&'a self, fixture_name: &str) -> Option<&'proj Fixture> {
        self.all_fixtures(None)
            .into_iter()
            .find(|fixture| fixture.name() == fixture_name)
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
