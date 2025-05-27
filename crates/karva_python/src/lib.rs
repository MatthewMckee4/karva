use karva::main as karva_main;
use pyo3::prelude::*;

/// Formats the sum of two numbers as string.
#[pyfunction]
fn sum_as_string(a: usize, b: usize) -> PyResult<String> {
    Ok((a + b).to_string())
}

#[pyfunction]
fn karva_run() -> PyResult<()> {
    karva_main();
    Ok(())
}

/// A Python module implemented in Rust.
#[pymodule]
fn _karva(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(sum_as_string, m)?)?;
    m.add_function(wrap_pyfunction!(karva_run, m)?)?;
    Ok(())
}
