//! Runtime binding and execution for statically collected Python doctests.

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyModule};

const DOCTEST_RUNTIME_CODE: &std::ffi::CStr = c"
import doctest
import textwrap
import traceback


class _Failure(AssertionError):
    def __init__(self, message, line):
        super().__init__(message)
        self.line = line


def _block(label, value):
    value = value.rstrip('\\n') or '<nothing>'
    indented = textwrap.indent(value, '  ')
    return f'{label}:\\n{indented}'


def _function(test):
    def run():
        fresh = doctest.DocTest(
            test.examples,
            test.globs,
            test.name,
            test.filename,
            test.lineno,
            test.docstring,
        )
        try:
            doctest.DebugRunner().run(fresh)
        except doctest.DocTestFailure as error:
            line = error.test.lineno + error.example.lineno + 1
            message = '\\n'.join(
                (
                    _block('Expected output', error.example.want),
                    _block('Actual output', error.got),
                )
            )
            raise _Failure(message, line) from None
        except doctest.UnexpectedException as error:
            line = error.test.lineno + error.example.lineno + 1
            exception = ''.join(
                traceback.format_exception_only(*error.exc_info[:2])
            )
            message = _block('Unexpected exception', exception)
            raise _Failure(message, line) from None

    return run


def _missing(reason):
    def run():
        import karva

        karva.skip(reason)

    return run


def _find(module):
    return {
        test.name: _function(test)
        for test in doctest.DocTestFinder().find(module)
        if test.examples
    }
";

static DOCTEST_RUNTIME: PyOnceLock<Py<PyDict>> = PyOnceLock::new();

fn runtime(py: Python<'_>) -> PyResult<&Bound<'_, PyDict>> {
    DOCTEST_RUNTIME
        .get_or_try_init(py, || {
            let namespace = PyDict::new(py);
            py.run(DOCTEST_RUNTIME_CODE, Some(&namespace), None)?;
            PyResult::Ok(namespace.unbind())
        })
        .map(|namespace| namespace.bind(py))
}

/// Returns zero-argument callables keyed by their stdlib doctest object name.
pub fn find_doctest_functions<'py>(
    py: Python<'py>,
    module: &Bound<'py, PyModule>,
) -> PyResult<Bound<'py, PyDict>> {
    runtime(py)?
        .get_item("_find")?
        .ok_or_else(|| PyRuntimeError::new_err("failed to load inline doctest finder"))?
        .call1((module,))?
        .cast_into::<PyDict>()
        .map_err(Into::into)
}

/// Returns a skipped placeholder for a source doctest unavailable after import.
pub fn missing_doctest_function<'py>(py: Python<'py>, reason: &str) -> PyResult<Bound<'py, PyAny>> {
    runtime(py)?
        .get_item("_missing")?
        .ok_or_else(|| PyRuntimeError::new_err("failed to load inline missing-doctest handler"))?
        .call1((reason,))
}

/// Returns the source line carried by a doctest execution failure.
pub fn failure_line(py: Python<'_>, error: &PyErr) -> Option<usize> {
    let failure = runtime(py).ok()?.get_item("_Failure").ok()??;
    if !error.matches(py, &failure).ok()? {
        return None;
    }
    error.value(py).getattr("line").ok()?.extract().ok()
}
