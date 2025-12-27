use karva_core::init_module;
use pyo3::prelude::*;

#[pymodule]
pub(crate) fn _karva(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    init_module(py, m)?;
    Ok(())
}
