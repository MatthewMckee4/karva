//! Python interpreter attachment helpers.
//!
//! Wraps [`pyo3::Python::attach`] with first-time interpreter initialization
//! and optional suppression of `sys.stdout` / `sys.stderr` to `/dev/null`
//! for the duration of the callback.

use std::ffi::CString;

use pyo3::prelude::*;

/// Initialize the Python interpreter (idempotent) and attach to it for the
/// duration of `f`.
fn attach<F, R>(f: F) -> R
where
    F: for<'py> FnOnce(Python<'py>) -> R,
{
    Python::initialize();
    Python::attach(f)
}

/// Like [`attach`], but redirects Python's `sys.stdout` and `sys.stderr` to
/// `/dev/null` for the duration of `f` when `show_output` is `false`.
///
/// If `/dev/null` cannot be opened we fall back to unsuppressed output rather
/// than failing the test run.
pub fn attach_with_output<F, R>(show_output: bool, f: F) -> R
where
    F: for<'py> FnOnce(Python<'py>) -> R,
{
    attach(|py| {
        if show_output {
            // The worker's stdout is a pipe to the orchestrator (see
            // `karva_runner::progress::OutputDrain`). Python defaults to
            // block-buffering when sys.stdout isn't a TTY, which delays
            // every `print()` until the buffer fills or the interpreter
            // shuts down — so the orchestrator only sees test output at
            // worker exit, after the reporter has already emitted result
            // lines. Forcing line-buffering here keeps prints flowing
            // alongside reporter output in real time.
            let _ = enable_line_buffering(py);
            return f(py);
        }

        let Ok(null_file) = open_devnull(py) else {
            return f(py);
        };

        let _ = redirect_stdio(py, &null_file);
        let result = f(py);
        let _ = flush_and_mute(py, &null_file);
        result
    })
}

/// Reconfigure `sys.stdout` and `sys.stderr` to line-buffer their writes.
///
/// Best-effort: silently no-ops if either stream isn't a `TextIOWrapper`
/// (e.g. when something earlier in the run replaced them with a custom
/// object that lacks `reconfigure`).
fn enable_line_buffering(py: Python<'_>) -> PyResult<()> {
    let code = CString::new(
        "import sys\n\
         for s in (sys.stdout, sys.stderr):\n\
         \x20   try:\n\
         \x20       s.reconfigure(line_buffering=True)\n\
         \x20   except (AttributeError, ValueError):\n\
         \x20       pass\n",
    )
    .expect("hardcoded code has no nul bytes");
    py.run(code.as_c_str(), None, None)
}

fn open_devnull(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    let os = py.import("os")?;
    let builtins = py.import("builtins")?;
    builtins
        .getattr("open")?
        .call1((os.getattr("devnull")?, "w"))
}

fn redirect_stdio<'py>(py: Python<'py>, null_file: &Bound<'py, PyAny>) -> PyResult<()> {
    let sys = py.import("sys")?;
    for stream in ["stdout", "stderr"] {
        sys.setattr(stream, null_file.clone())?;
    }
    Ok(())
}

/// Close whatever is currently on `sys.stdout`/`sys.stderr` (so pending writes
/// flush) and reset both to `null_file`. We don't restore the originals — the
/// runner doesn't emit to real stdout after the callback returns, and a test
/// may have swapped the streams itself.
fn flush_and_mute<'py>(py: Python<'py>, null_file: &Bound<'py, PyAny>) -> PyResult<()> {
    let sys = py.import("sys")?;
    for stream in ["stdout", "stderr"] {
        sys.getattr(stream)?.call_method0("close")?;
        sys.setattr(stream, null_file.clone())?;
    }
    Ok(())
}
