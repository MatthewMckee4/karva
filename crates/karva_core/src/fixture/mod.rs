use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

use karva_project::{path::SystemPathBuf, utils::module_name};
use pyo3::prelude::*;
use ruff_python_ast::{Decorator, Expr, StmtFunctionDef};

use crate::{fixture::python::FixtureFunctionDefinition, utils::recursive_add_to_sys_path};

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

impl From<String> for FixtureScope {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

#[must_use]
pub fn check_valid_scope(scope: &str) -> bool {
    matches!(scope, "module" | "session" | "function" | "package")
}

#[derive(Debug)]
pub struct Fixture {
    pub name: String,
    pub function_def: StmtFunctionDef,
    pub scope: FixtureScope,
    pub function: Py<FixtureFunctionDefinition>,
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

    pub fn from(
        py: &Python<'_>,
        val: StmtFunctionDef,
        path: &SystemPathBuf,
        cwd: &SystemPathBuf,
    ) -> Result<Self, String> {
        if !is_fixture_function(&val) {
            return Err(format!("Function {} is not a fixture", val.name));
        }

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

        Ok(Self::new(
            val.name.to_string(),
            val,
            FixtureScope::from(scope),
            py_function.into(),
        ))
    }

    pub fn call(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.function.call(py, (), None)
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

pub struct TestCaseFixtures<'a> {
    session: &'a HashMap<String, Py<PyAny>>,
    module: &'a HashMap<String, Py<PyAny>>,
    function: &'a HashMap<String, Py<PyAny>>,
}

impl<'a> TestCaseFixtures<'a> {
    #[must_use]
    pub const fn new(
        session: &'a HashMap<String, Py<PyAny>>,
        module: &'a HashMap<String, Py<PyAny>>,
        function: &'a HashMap<String, Py<PyAny>>,
    ) -> Self {
        Self {
            session,
            module,
            function,
        }
    }

    #[must_use]
    pub fn get_fixture(&self, fixture_name: &str) -> Option<&Py<PyAny>> {
        self.session
            .get(fixture_name)
            .or_else(|| self.module.get(fixture_name))
            .or_else(|| self.function.get(fixture_name))
    }
}
