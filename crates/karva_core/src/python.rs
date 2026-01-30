use pyo3::prelude::*;
use pyo3::wrap_pymodule;

use crate::extensions::fixtures::MockEnv;
use crate::extensions::fixtures::python::{
    FixtureFunctionDefinition, FixtureFunctionMarker, InvalidFixtureError, fixture_decorator,
};
use crate::extensions::functions::{FailError, SkipError, fail, param, skip};
use crate::extensions::tags::python::{PyTags, PyTestFunction, tags};

pub fn init_module(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(fixture_decorator, m)?)?;
    m.add_function(wrap_pyfunction!(skip, m)?)?;
    m.add_function(wrap_pyfunction!(fail, m)?)?;
    m.add_function(wrap_pyfunction!(param, m)?)?;

    m.add_class::<FixtureFunctionMarker>()?;
    m.add_class::<FixtureFunctionDefinition>()?;
    m.add_class::<PyTags>()?;
    m.add_class::<PyTestFunction>()?;
    m.add_class::<MockEnv>()?;

    m.add_wrapped(wrap_pymodule!(tags))?;

    m.add("SkipError", py.get_type::<SkipError>())?;
    m.add("FailError", py.get_type::<FailError>())?;
    m.add("InvalidFixtureError", py.get_type::<InvalidFixtureError>())?;
    Ok(())
}
