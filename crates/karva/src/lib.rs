use karva_cli::karva_main;
use karva_core::init_module;
use pyo3::prelude::*;

#[pyfunction]
pub(crate) fn karva_run() -> i32 {
    karva_main(|args| {
        let mut args: Vec<_> = args.into_iter().skip(1).collect();
        if !args.is_empty() {
            if let Some(arg) = args.first() {
                if arg.to_string_lossy() == "python" {
                    args.remove(0);
                }
            }
        }
        args
    })
    .to_i32()
}

#[pymodule]
pub(crate) fn _karva(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(karva_run, m)?)?;
    init_module(py, m)?;
    Ok(())
}
