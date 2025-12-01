use pyo3::{prelude::*, wrap_pymodule};

use crate::extensions::{
    fixtures::{
        Mock,
        python::{
            FixtureFunctionDefinition, FixtureFunctionMarker, FixtureRequest, fixture_decorator,
        },
    },
    tags::python::{FailError, PyTags, PyTestFunction, SkipError, fail, skip, tags},
};

pub fn init_module(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(fixture_decorator, m)?)?;
    m.add_function(wrap_pyfunction!(skip, m)?)?;
    m.add_function(wrap_pyfunction!(fail, m)?)?;
    m.add_class::<FixtureFunctionMarker>()?;
    m.add_class::<FixtureFunctionDefinition>()?;
    m.add_class::<FixtureRequest>()?;
    m.add_class::<PyTags>()?;
    m.add_class::<PyTestFunction>()?;
    m.add_class::<Mock>()?;
    m.add_wrapped(wrap_pymodule!(tags))?;
    m.add("SkipError", py.get_type::<SkipError>())?;
    m.add("FailError", py.get_type::<FailError>())?;
    Ok(())
}
