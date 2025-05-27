use pyo3::prelude::*;

use karva::main as karva_main;

#[pyfunction]
pub fn karva_run() -> i32 {
    karva_main().to_i32()
}

#[pymodule]
fn _karva(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_wrapped(wrap_pyfunction!(karva_run))?;
    Ok(())
}
