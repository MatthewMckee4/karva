use pyo3::prelude::*;
use pyo3::wrap_pymodule;

use crate::extensions::fixtures::MockEnv;
use crate::extensions::fixtures::python::{
    FixtureFunctionDefinition, FixtureFunctionMarker, InvalidFixtureError, fixture_decorator,
};
use crate::extensions::functions::raises::raises;
use crate::extensions::functions::snapshot::assert_snapshot;
use crate::extensions::functions::{
    ExceptionInfo, FailError, RaisesContext, SkipError, SnapshotMismatchError, fail, param, skip,
};
use crate::extensions::tags::python::{PyTags, PyTestFunction, tags};

pub fn init_module(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(fixture_decorator, m)?)?;
    m.add_function(wrap_pyfunction!(skip, m)?)?;
    m.add_function(wrap_pyfunction!(fail, m)?)?;
    m.add_function(wrap_pyfunction!(param, m)?)?;
    m.add_function(wrap_pyfunction!(raises, m)?)?;
    m.add_function(wrap_pyfunction!(assert_snapshot, m)?)?;

    m.add_class::<FixtureFunctionMarker>()?;
    m.add_class::<FixtureFunctionDefinition>()?;
    m.add_class::<PyTags>()?;
    m.add_class::<PyTestFunction>()?;
    m.add_class::<MockEnv>()?;
    m.add_class::<ExceptionInfo>()?;
    m.add_class::<RaisesContext>()?;

    m.add_wrapped(wrap_pymodule!(tags))?;

    m.add("SkipError", py.get_type::<SkipError>())?;
    m.add("FailError", py.get_type::<FailError>())?;
    m.add("InvalidFixtureError", py.get_type::<InvalidFixtureError>())?;
    m.add(
        "SnapshotMismatchError",
        py.get_type::<SnapshotMismatchError>(),
    )?;
    Ok(())
}
