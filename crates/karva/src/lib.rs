use karva_cli::karva_main;
use karva_core::fixture::check_valid_scope;
use pyo3::{exceptions::PyValueError, prelude::*};

mod fixture;
use fixture::{FixtureFunctionDefinition, FixtureFunctionMarker};

#[pyfunction]
#[must_use]
pub fn karva_run() -> i32 {
    karva_main(|args| {
        let mut args: Vec<_> = args.into_iter().skip(1).collect();
        if !args.is_empty() && args[0].to_string_lossy() == "python" {
            args.remove(0);
        }
        args
    })
    .to_i32()
}

#[pyfunction(name = "fixture")]
#[pyo3(signature = (func=None, *, scope="function", name=None))]
pub fn fixture_decorator(
    py: Python<'_>,
    func: Option<PyObject>,
    scope: &str,
    name: Option<&str>,
) -> PyResult<PyObject> {
    if !check_valid_scope(scope) {
        return Err(PyValueError::new_err("Invalid scope"));
    }
    let marker = FixtureFunctionMarker::new(scope.to_string(), name.map(String::from));
    if let Some(f) = func {
        let fixture_def = marker.call_with_function(py, f)?;
        Ok(Py::new(py, fixture_def)?.into_any())
    } else {
        Ok(Py::new(py, marker)?.into_any())
    }
}

#[pymodule]
pub fn _karva(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(karva_run, m)?)?;
    m.add_function(wrap_pyfunction!(fixture_decorator, m)?)?;
    m.add_class::<FixtureFunctionMarker>()?;
    m.add_class::<FixtureFunctionDefinition>()?;
    Ok(())
}
