use pyo3::{prelude::*, wrap_pymodule};

use crate::extensions::{
    fixtures::{
        MockEnv,
        python::{
            FixtureFunctionDefinition, FixtureFunctionMarker, FixtureRequest, InvalidFixtureError,
            fixture_decorator,
        },
    },
    functions::{FailError, SkipError, fail, param, skip},
    tags::python::{PyTags, PyTestFunction, tags},
};

pub fn init_module(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(fixture_decorator, m)?)?;
    m.add_function(wrap_pyfunction!(skip, m)?)?;
    m.add_function(wrap_pyfunction!(fail, m)?)?;
    m.add_function(wrap_pyfunction!(param, m)?)?;

    m.add_class::<FixtureFunctionMarker>()?;
    m.add_class::<FixtureFunctionDefinition>()?;
    m.add_class::<FixtureRequest>()?;
    m.add_class::<PyTags>()?;
    m.add_class::<PyTestFunction>()?;
    m.add_class::<MockEnv>()?;

    m.add_wrapped(wrap_pymodule!(tags))?;

    m.add("SkipError", py.get_type::<SkipError>())?;
    m.add("FailError", py.get_type::<FailError>())?;
    m.add("InvalidFixtureError", py.get_type::<InvalidFixtureError>())?;
    Ok(())
}
