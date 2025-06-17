use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    hash::{Hash, Hasher},
};

use pyo3::{prelude::*, types::PyTuple};
use ruff_python_ast::{Decorator, Expr, StmtFunctionDef};

mod extractor;
mod manager;
pub mod python;

pub use extractor::FixtureExtractor;
pub use manager::FixtureManager;

use crate::utils::Upcast;

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

impl HasFunctionDefinition for Fixture {
    fn function_definition(&self) -> &StmtFunctionDef {
        &self.function_def
    }

    fn name(&self) -> &str {
        &self.name
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

impl<'a> Upcast<Vec<&'a dyn UsesFixture>> for Vec<&'a Fixture> {
    fn upcast(self) -> Vec<&'a dyn UsesFixture> {
        self.into_iter().map(|tc| tc as &dyn UsesFixture).collect()
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

    fn name(&self) -> &str;
}

pub trait UsesFixture: std::fmt::Debug {
    #[must_use]
    fn uses_fixture(&self, fixture_name: &str) -> bool;

    #[must_use]
    fn dependencies(&self) -> Vec<String>;

    fn name(&self) -> &str;
}

impl<T: HasFunctionDefinition + std::fmt::Debug> UsesFixture for T {
    fn uses_fixture(&self, fixture_name: &str) -> bool {
        self.get_required_fixture_names()
            .contains(&fixture_name.to_string())
    }

    fn dependencies(&self) -> Vec<String> {
        self.get_required_fixture_names()
    }

    fn name(&self) -> &str {
        self.name()
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

pub type CalledFixtures<'a> = HashMap<String, Bound<'a, PyAny>>;

/// This trait is used to get all direct fixtures used the current scope.
///
/// For example, if we are in a test module, we want to get all fixtures used in the test module.
/// If we are in a package, we want to get all fixtures used in the package from the configuration module.
pub trait HasFixtures<'proj>: std::fmt::Debug {
    fn fixtures<'a: 'proj>(
        &'a self,
        scope: &[FixtureScope],
        test_cases: Vec<&dyn UsesFixture>,
    ) -> Vec<&'proj Fixture> {
        let mut graph = Vec::new();
        for fixture in self.all_fixtures(test_cases) {
            if scope.contains(fixture.scope()) {
                graph.push(fixture);
            }
        }
        graph
    }

    fn get_fixture<'a: 'proj>(&'a self, fixture_name: &str) -> Option<&'proj Fixture> {
        self.all_fixtures(Vec::new())
            .into_iter()
            .find(|fixture| fixture.name() == fixture_name)
    }

    fn all_fixtures<'a: 'proj>(&'a self, test_cases: Vec<&dyn UsesFixture>) -> Vec<&'proj Fixture>;
}

impl<'proj> HasFixtures<'proj> for Vec<&dyn HasFixtures<'proj>> {
    fn all_fixtures<'a: 'proj>(&'a self, test_cases: Vec<&dyn UsesFixture>) -> Vec<&'proj Fixture> {
        self.iter()
            .flat_map(|p| p.all_fixtures(test_cases.clone()))
            .collect::<Vec<&'proj Fixture>>()
    }
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

#[derive(Debug, Default, Clone)]
pub struct FixtureDependencyGraph<'proj> {
    fixture_graph: HashMap<&'proj Fixture, HashSet<&'proj Fixture>>,
}

impl<'proj> FixtureDependencyGraph<'proj> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            fixture_graph: HashMap::new(),
        }
    }

    pub fn add_fixture(&mut self, fixture: &'proj Fixture) {
        self.fixture_graph.entry(fixture).or_default();
    }

    pub fn add_dependency(&mut self, fixture: &'proj Fixture, dependency: &'proj Fixture) {
        let deps = self.fixture_graph.entry(fixture).or_default();
        deps.insert(dependency);
    }

    pub fn update(&mut self, other: Self) {
        for (fixture, dependencies) in other.fixture_graph {
            self.fixture_graph
                .entry(fixture)
                .and_modify(|deps| deps.extend(dependencies.clone()))
                .or_insert_with(|| dependencies.clone());
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'proj Fixture, &HashSet<&'proj Fixture>)> {
        self.fixture_graph.iter().map(|(k, v)| (*k, v))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fixture_graph.is_empty()
    }

    #[must_use]
    pub fn get(&self, fixture: &'proj Fixture) -> Option<&HashSet<&'proj Fixture>> {
        self.fixture_graph.get(fixture)
    }
}

impl<'proj> IntoIterator for FixtureDependencyGraph<'proj> {
    type Item = &'proj Fixture;
    type IntoIter = std::collections::hash_map::IntoKeys<&'proj Fixture, HashSet<&'proj Fixture>>;

    fn into_iter(self) -> Self::IntoIter {
        self.fixture_graph.into_keys()
    }
}

impl<'proj> FromIterator<&'proj Fixture> for FixtureDependencyGraph<'proj> {
    fn from_iter<T: IntoIterator<Item = &'proj Fixture>>(iter: T) -> Self {
        let mut graph = Self::new();
        for fixture in iter {
            graph.add_fixture(fixture);
        }
        graph
    }
}
