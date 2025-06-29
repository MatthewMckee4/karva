use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::fixture::{Fixture, FixtureScope, python::FixtureFunctionDefinition};

#[derive(Default)]
pub struct FixtureExtractor {}

impl FixtureExtractor {
    #[must_use]
    pub const fn new() -> Self {
        Self {}
    }

    pub fn try_from_pytest_fixture(
        function_def: StmtFunctionDef,
        function: &Bound<'_, PyAny>,
    ) -> Result<Fixture, String> {
        let found_name = function
            .getattr("_fixture_function_marker")
            .map_err(|e| e.to_string())?
            .getattr("name")
            .map_err(|e| e.to_string())?;

        let name = if found_name.is_none() {
            function_def.name.to_string()
        } else {
            found_name.to_string()
        };

        let scope = function
            .getattr("_fixture_function_marker")
            .map_err(|e| e.to_string())?
            .getattr("scope")
            .map_err(|e| e.to_string())?;

        let function = function
            .getattr("_fixture_function")
            .map_err(|e| e.to_string())?;

        Ok(Fixture::new(
            name,
            function_def,
            FixtureScope::try_from(scope.to_string())?,
            function.into(),
        ))
    }

    pub fn try_from_function(
        val: &StmtFunctionDef,
        py_module: &Bound<'_, PyModule>,
    ) -> Result<Fixture, String> {
        let function = py_module
            .getattr(val.name.to_string())
            .map_err(|e| e.to_string())?;

        let Ok(py_function) = function
            .clone()
            .downcast_into::<FixtureFunctionDefinition>()
        else {
            return Self::try_from_pytest_fixture(val.clone(), &function);
        };

        let scope = py_function.borrow_mut().scope.clone();
        let name = py_function.borrow_mut().name.clone();

        Ok(Fixture::new(
            name,
            val.clone(),
            FixtureScope::try_from(scope)?,
            py_function.into(),
        ))
    }
}
