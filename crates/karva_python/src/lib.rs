//! `PyO3` bridge from Python package entry points to controller and worker runtimes.

use std::ffi::OsString;

use karva::karva_main;
use karva_test_semantic::init_module;
use pyo3::prelude::*;

/// Skip the interpreter path and an optional leading `"python"` argument.
///
/// When invoked via `python -m karva`, the arg list starts with the
/// interpreter path followed by `"python"` — both need to be stripped
/// before the real CLI parser sees the arguments.
fn filter_args(args: Vec<OsString>) -> Vec<OsString> {
    let mut args: Vec<_> = args.into_iter().skip(1).collect();
    if let Some(arg) = args.first() {
        if arg.to_string_lossy() == "python" {
            args.remove(0);
        }
    }
    args
}

#[pyfunction]
/// Runs the shared CLI and reuses this installed launcher for worker subprocesses.
pub(crate) fn karva_run(py: Python<'_>) -> PyResult<i32> {
    let launcher = py.import("sys")?.getattr("argv")?.get_item(0)?.extract()?;
    Ok(karva_main(filter_args, Some(launcher)).to_i32())
}

#[pymodule]
/// Registers CLI entry points and Karva's Python-facing testing API.
fn _karva(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(karva_run, m)?)?;
    init_module(py, m)?;
    Ok(())
}
