use std::collections::HashMap;
use std::fmt::Write;

use camino::Utf8Path;
use karva_static::WorkerEnvVars;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyAnyMethods, PyDict};
use pyo3::{PyResult, Python};

const MAKE_SYNC_CODE: &std::ffi::CStr = c"
def _make_sync(async_fn):
    import asyncio, functools
    @functools.wraps(async_fn)
    def wrapper(*args, **kwargs):
        return asyncio.run(async_fn(*args, **kwargs))
    return wrapper
";

/// Runs a Python coroutine to completion using `asyncio.run()`.
pub(crate) fn run_coroutine(py: Python<'_>, coroutine: Py<PyAny>) -> PyResult<Py<PyAny>> {
    let asyncio = py.import("asyncio")?;
    Ok(asyncio.call_method1("run", (coroutine,))?.unbind())
}

/// Runs a Python test with a timeout, raising `TimeoutError` if it does not
/// finish in time.
///
/// Sync tests are submitted to a single-worker `ThreadPoolExecutor`; if the
/// future does not complete within `seconds`, the still-running thread is
/// abandoned (Python has no safe way to interrupt arbitrary code) and the
/// executor is shut down without waiting. Async tests are wrapped in
/// `asyncio.wait_for`, which cancels the coroutine on timeout.
pub(crate) fn run_test_with_timeout(
    py: Python<'_>,
    function: &Py<PyAny>,
    kwargs: &HashMap<String, Py<PyAny>>,
    is_async: bool,
    seconds: f64,
) -> PyResult<Py<PyAny>> {
    let kwargs_dict = PyDict::new(py);
    for (key, value) in kwargs {
        kwargs_dict.set_item(key, value)?;
    }

    if is_async {
        run_async_with_timeout(py, function, &kwargs_dict, seconds)
    } else {
        run_sync_with_timeout(py, function, &kwargs_dict, seconds)
    }
}

fn run_sync_with_timeout(
    py: Python<'_>,
    function: &Py<PyAny>,
    kwargs_dict: &Bound<'_, PyDict>,
    seconds: f64,
) -> PyResult<Py<PyAny>> {
    let concurrent_futures = py.import("concurrent.futures")?;
    let timeout_class = concurrent_futures.getattr("TimeoutError")?;
    let executor = concurrent_futures
        .getattr("ThreadPoolExecutor")?
        .call1((1u32,))?;

    let future = executor.call_method("submit", (function,), Some(kwargs_dict))?;
    let result = future.call_method1("result", (seconds,));

    let shutdown_kwargs = PyDict::new(py);
    shutdown_kwargs.set_item("wait", false)?;
    executor.call_method("shutdown", (), Some(&shutdown_kwargs))?;

    rebrand_timeout_error(py, &timeout_class, result.map(pyo3::Bound::unbind), seconds)
}

fn run_async_with_timeout(
    py: Python<'_>,
    function: &Py<PyAny>,
    kwargs_dict: &Bound<'_, PyDict>,
    seconds: f64,
) -> PyResult<Py<PyAny>> {
    let asyncio = py.import("asyncio")?;
    let timeout_class = asyncio.getattr("TimeoutError")?;
    let coroutine = function.call(py, (), Some(kwargs_dict))?;
    let wait_for = asyncio.call_method1("wait_for", (coroutine, seconds))?;
    rebrand_timeout_error(
        py,
        &timeout_class,
        asyncio
            .call_method1("run", (wait_for,))
            .map(pyo3::Bound::unbind),
        seconds,
    )
}

/// Replace a `TimeoutError` raised from inside `concurrent.futures` or
/// `asyncio` with one that has no traceback, so the test failure diagnostic
/// points at the test function instead of at framework internals.
///
/// `timeout_class` is the path-specific timeout exception class
/// (`concurrent.futures.TimeoutError` for sync, `asyncio.TimeoutError` for
/// async). On Python >= 3.11 both are aliases of the builtin `TimeoutError`,
/// but on 3.10 they are distinct classes — checking the imported class is
/// version-portable.
fn rebrand_timeout_error(
    py: Python<'_>,
    timeout_class: &Bound<'_, PyAny>,
    result: PyResult<Py<PyAny>>,
    seconds: f64,
) -> PyResult<Py<PyAny>> {
    match result {
        Ok(v) => Ok(v),
        Err(err) => {
            let is_timeout = match err.matches(py, timeout_class) {
                Ok(is_timeout) => is_timeout,
                Err(match_err) => {
                    tracing::warn!("Failed to classify timeout exception: {match_err}");
                    false
                }
            };
            if is_timeout {
                Err(pyo3::exceptions::PyTimeoutError::new_err(format!(
                    "Test exceeded timeout of {seconds} seconds"
                )))
            } else {
                Err(err)
            }
        }
    }
}

/// Patches an async test function wrapped by a sync decorator (e.g. Hypothesis `@given`).
///
/// When `@given` decorates an `async def test_*()`, Hypothesis wraps it in a sync callable
/// and stores the original async function at `function.hypothesis.inner_test`. Without
/// patching, Hypothesis calls the async function directly, gets a coroutine, and raises
/// `InvalidArgument` because it cannot await it.
///
/// This function detects that situation and replaces `inner_test` with a sync wrapper
/// that uses `asyncio.run()`, following the Hypothesis-documented pattern for test runners.
///
/// Returns `true` if the function was patched (caller should NOT apply `asyncio.run()`),
/// or `false` if no patching was needed.
pub(crate) fn patch_async_test_function(py: Python<'_>, function: &Py<PyAny>) -> PyResult<bool> {
    let asyncio = py.import("asyncio")?;
    let is_coroutine_fn = asyncio
        .call_method1("iscoroutinefunction", (function,))?
        .extract::<bool>()?;

    // The callable itself is async — no decorator wrapping, use normal asyncio.run() path.
    if is_coroutine_fn {
        return Ok(false);
    }

    // The callable is sync (wrapped by a decorator). Check for Hypothesis inner_test.
    let Ok(hypothesis_attr) = function.getattr(py, "hypothesis") else {
        return Ok(false);
    };
    let Ok(inner_test) = hypothesis_attr.getattr(py, "inner_test") else {
        return Ok(false);
    };

    let inner_is_async = asyncio
        .call_method1("iscoroutinefunction", (&inner_test,))?
        .extract::<bool>()?;

    if !inner_is_async {
        return Ok(false);
    }

    // Replace inner_test with a sync wrapper that uses asyncio.run().
    // Uses inline Python because PyCFunction closures lack the signature metadata and
    // calling conventions that Hypothesis requires to introspect and invoke inner_test.
    let locals = pyo3::types::PyDict::new(py);
    py.run(MAKE_SYNC_CODE, None, Some(&locals))?;
    let make_sync = locals.get_item("_make_sync")?.ok_or_else(|| {
        PyRuntimeError::new_err("failed to load async test wrapper from inline Python")
    })?;
    let sync_wrapper = make_sync.call1((inner_test,))?;
    hypothesis_attr.setattr(py, "inner_test", sync_wrapper)?;

    Ok(true)
}

/// Sets `KARVA_ATTEMPT` and `KARVA_TOTAL_ATTEMPTS` on Python's `os.environ` so
/// the currently running test can read them.
pub(crate) fn set_attempt_env(py: Python<'_>, attempt: u32, total_attempts: u32) -> PyResult<()> {
    let environ = py.import("os")?.getattr("environ")?;
    environ.set_item(WorkerEnvVars::KARVA_ATTEMPT, attempt.to_string())?;
    environ.set_item(
        WorkerEnvVars::KARVA_TOTAL_ATTEMPTS,
        total_attempts.to_string(),
    )?;
    Ok(())
}

/// Sets `KARVA_TEST_NAME` on Python's `os.environ` to the qualified name of
/// the currently running test variant.
pub(crate) fn set_test_name_env(py: Python<'_>, qualified_name: &str) -> PyResult<()> {
    let environ = py.import("os")?.getattr("environ")?;
    environ.set_item(WorkerEnvVars::KARVA_TEST_NAME, qualified_name)?;
    Ok(())
}

/// Adds a directory path to Python's sys.path at the specified index.
pub(crate) fn add_to_sys_path(py: Python<'_>, path: &Utf8Path, index: isize) -> PyResult<()> {
    let sys_module = py.import("sys")?;
    let sys_path = sys_module.getattr("path")?;
    sys_path.call_method1("insert", (index, path.to_string()))?;
    Ok(())
}

pub(crate) fn full_test_name(
    py: Python,
    function: String,
    kwargs: &HashMap<String, Py<PyAny>>,
) -> String {
    if kwargs.is_empty() {
        function
    } else {
        let mut args_str = String::new();
        let mut sorted_kwargs: Vec<_> = kwargs.iter().collect();
        sorted_kwargs.sort_by_key(|(key, _)| &**key);

        for (i, (key, value)) in sorted_kwargs.iter().enumerate() {
            if i > 0 {
                args_str.push_str(", ");
            }
            if let Ok(value) = value.cast_bound::<PyAny>(py) {
                let trimmed_value_str = truncate_string(&value.to_string());
                let truncated_key = truncate_string(key);
                let _ = write!(args_str, "{truncated_key}={trimmed_value_str}");
            }
        }
        format!("{function}({args_str})")
    }
}

/// Maximum display length for parameter keys and values in test names.
///
/// Keeps parameterized test names (e.g., `test_foo(key=value)`) readable in
/// CLI output by truncating long values with an ellipsis.
const TRUNCATE_LENGTH: usize = 30;

pub(crate) fn truncate_string(value: &str) -> String {
    if value.chars().count() > TRUNCATE_LENGTH {
        let truncated: String = value.chars().take(TRUNCATE_LENGTH - 3).collect();
        format!("{truncated}...")
    } else {
        value.to_string()
    }
}
