use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::extensions::fixtures::{Fixture, FixtureScope, python::FixtureFunctionDefinition};

fn get_attribute<'a>(function: Bound<'a, PyAny>, attributes: &[&str]) -> Option<Bound<'a, PyAny>> {
    let mut current = function;
    for attribute in attributes {
        let current_attr = current.getattr(attribute).ok()?;
        current = current_attr;
    }
    Some(current.clone())
}

pub fn try_from_pytest_function(
    function_definition: &StmtFunctionDef,
    function: &Bound<'_, PyAny>,
    is_generator_function: bool,
) -> Result<Option<Fixture>, String> {
    let Some(found_name) = get_attribute(function.clone(), &["_fixture_function_marker", "name"])
    else {
        return Ok(None);
    };

    let Some(scope) = get_attribute(function.clone(), &["_fixture_function_marker", "scope"])
    else {
        return Ok(None);
    };

    let Some(auto_use) = get_attribute(function.clone(), &["_fixture_function_marker", "autouse"])
    else {
        return Ok(None);
    };

    let Some(function) = get_attribute(function.clone(), &["_fixture_function"]) else {
        return Ok(None);
    };

    let name = if found_name.is_none() {
        function_definition.name.to_string()
    } else {
        found_name.to_string()
    };

    Ok(Some(Fixture::new(
        name,
        function_definition.clone(),
        FixtureScope::try_from(scope.to_string())?,
        auto_use.extract::<bool>().unwrap_or(false),
        function.into(),
        is_generator_function,
    )))
}

pub fn try_from_karva_function(
    function_def: &StmtFunctionDef,
    function: &Bound<'_, PyAny>,
    is_generator_function: bool,
) -> Result<Option<Fixture>, String> {
    let Ok(py_function) = function
        .clone()
        .downcast_into::<FixtureFunctionDefinition>()
    else {
        return Ok(None);
    };

    let py_function_borrow = py_function.borrow_mut();

    let scope = py_function_borrow.scope.clone();
    let name = py_function_borrow.name.clone();
    let auto_use = py_function_borrow.auto_use;

    Ok(Some(Fixture::new(
        name,
        function_def.clone(),
        FixtureScope::try_from(scope)?,
        auto_use,
        py_function.into(),
        is_generator_function,
    )))
}
