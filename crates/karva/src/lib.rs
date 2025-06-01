use karva_cli::karva_main;
use pyo3::prelude::*;

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

#[pymodule]
pub fn _karva(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_wrapped(wrap_pyfunction!(karva_run))?;
    Ok(())
}
