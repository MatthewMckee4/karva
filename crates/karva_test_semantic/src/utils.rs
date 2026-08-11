use std::fmt::Write;

use camino::Utf8Path;
use karva_static::WorkerEnvVars;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyAnyMethods, PyCFunction, PyDict, PyString, PyTuple};
use pyo3::{PyResult, Python};
use ruff_python_ast::Parameters;

use crate::extensions::functions::snapshot::{
    SnapshotContext, capture_snapshot_thread_state, set_snapshot_thread_state,
};
use crate::runner::FixtureArguments;

/// Drives a coroutine with `asyncio.run()` while watching the event loop for
/// exceptions that never reached the awaiting code.
///
/// A background task that fails without being awaited cannot propagate through
/// the test coroutine, so the loop reports it to its exception handler instead.
/// Installing a handler from inside the running loop turns those reports into a
/// raised exception, which lets an ordinary failure path attribute them to the
/// test or fixture that was running.
///
/// The handler is deliberately not restored: the report for an unretrieved task
/// arrives while `asyncio.run` is shutting the loop down, after the wrapper
/// coroutine has already returned. Restoring inside the wrapper would send those
/// late reports to the default handler and lose them. Nothing is leaked by
/// leaving it in place, because `asyncio.run` builds a fresh loop per call and
/// closes it afterwards, which also isolates parameter cases, retries, and
/// workers from one another.
///
/// Exceptions the test handled itself (awaited, or read through
/// `task.exception()`) never reach the handler, and neither does the clean
/// cancellation `asyncio.run` performs on still-pending tasks during shutdown.
const RUN_COROUTINE_CODE: &std::ffi::CStr = c"
import asyncio


class UnhandledBackgroundException(RuntimeError):
    '''Work started by a test failed without anything awaiting it.'''


def _describe(context):
    exception = context.get('exception')
    if exception is None:
        return context.get('message') or 'unknown asyncio error'

    description = f'{type(exception).__name__}: {exception}'
    source = context.get('future') or context.get('task') or context.get('handle')
    get_name = getattr(source, 'get_name', None)
    if get_name is None:
        return description
    try:
        return f'{get_name()}: {description}'
    except Exception:
        return description


def _build_error(contexts):
    descriptions = [_describe(context) for context in contexts]
    if len(descriptions) == 1:
        message = f'Unhandled exception in background task: {descriptions[0]}'
    else:
        joined = '\\n'.join(f'  [{i}] {d}' for i, d in enumerate(descriptions, 1))
        message = (
            f'{len(descriptions)} unhandled exceptions in background tasks:\\n{joined}'
        )
    error = UnhandledBackgroundException(message)
    # Chain the first real exception so the traceback points at the failing
    # background code rather than at this helper.
    for context in contexts:
        exception = context.get('exception')
        if exception is not None:
            error.__cause__ = exception
            break
    return error


def _run(coroutine):
    contexts = []

    async def _wrapper():
        asyncio.get_running_loop().set_exception_handler(
            lambda loop, context: contexts.append(context)
        )
        return await coroutine

    result = asyncio.run(_wrapper())
    if contexts:
        raise _build_error(contexts)
    return result


def _make_sync(async_fn):
    import functools

    @functools.wraps(async_fn)
    def wrapper(*args, **kwargs):
        return _run(async_fn(*args, **kwargs))

    return wrapper
";

/// Compiled [`RUN_COROUTINE_CODE`] namespace, built once per interpreter.
///
/// `run_coroutine` runs for every async test and every async fixture setup and
/// teardown, so recompiling the source per call would be wasted work on a hot
/// path.
static ASYNC_RUNTIME: PyOnceLock<Py<PyDict>> = PyOnceLock::new();

fn async_runtime(py: Python<'_>) -> PyResult<&Bound<'_, PyDict>> {
    ASYNC_RUNTIME
        .get_or_try_init(py, || {
            // The namespace doubles as globals so the helpers resolve the
            // module-level `asyncio` import through their `__globals__`.
            let namespace = PyDict::new(py);
            py.run(RUN_COROUTINE_CODE, Some(&namespace), None)?;
            PyResult::Ok(namespace.unbind())
        })
        .map(|namespace| namespace.bind(py))
}

fn async_runtime_attr<'py>(py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyAny>> {
    async_runtime(py)?.get_item(name)?.ok_or_else(|| {
        PyRuntimeError::new_err(format!("failed to load `{name}` from inline Python"))
    })
}

/// Runs a Python coroutine to completion, failing if work it started in the
/// background raised without anything awaiting it.
///
/// See [`RUN_COROUTINE_CODE`] for why the loop's exception handler is the
/// mechanism used to notice those failures.
pub fn run_coroutine(py: Python<'_>, coroutine: Py<PyAny>) -> PyResult<Py<PyAny>> {
    Ok(async_runtime_attr(py, "_run")?
        .call1((coroutine,))?
        .unbind())
}

/// Runs a Python test with a timeout, raising `TimeoutError` if it does not
/// finish in time.
///
/// Sync tests are submitted to a single-worker `ThreadPoolExecutor`; if the
/// future does not complete within `seconds`, the still-running thread is
/// abandoned (Python has no safe way to interrupt arbitrary code) and the
/// executor is shut down without waiting. Async tests are wrapped in
/// `asyncio.wait_for`, which cancels the coroutine on timeout.
pub fn run_test_with_timeout(
    py: Python<'_>,
    function: &Py<PyAny>,
    kwargs: &FixtureArguments,
    is_async: bool,
    seconds: f64,
    snapshot_context: &SnapshotContext,
) -> PyResult<Py<PyAny>> {
    let kwargs_dict = kwargs.to_kwargs(py)?;
    if is_async {
        run_async_with_timeout(py, function, &kwargs_dict, seconds)
    } else {
        run_sync_with_timeout(py, function, &kwargs_dict, seconds, snapshot_context)
    }
}

fn run_sync_with_timeout(
    py: Python<'_>,
    function: &Py<PyAny>,
    kwargs_dict: &Bound<'_, PyDict>,
    seconds: f64,
    snapshot_context: &SnapshotContext,
) -> PyResult<Py<PyAny>> {
    let concurrent_futures = py.import("concurrent.futures")?;
    let timeout_class = concurrent_futures.getattr("TimeoutError")?;
    let snapshot_state = capture_snapshot_thread_state(snapshot_context.clone());
    let initializer = PyCFunction::new_closure(
        py,
        None,
        None,
        move |_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| -> PyResult<()> {
            set_snapshot_thread_state(snapshot_state.clone());
            Ok(())
        },
    )?;
    let executor_kwargs = PyDict::new(py);
    executor_kwargs.set_item("initializer", initializer)?;
    let executor = concurrent_futures
        .getattr("ThreadPoolExecutor")?
        .call((1u32,), Some(&executor_kwargs))?;

    let copied_context = py.import("contextvars")?.call_method0("copy_context")?;
    let future = executor.call_method(
        "submit",
        (copied_context.getattr("run")?, function),
        Some(kwargs_dict),
    )?;
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
        run_coroutine(py, wait_for.unbind()),
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
pub fn patch_async_test_function(py: Python<'_>, function: &Py<PyAny>) -> PyResult<bool> {
    let inspect = py.import("inspect")?;
    let is_coroutine_fn = inspect
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

    let inner_is_async = inspect
        .call_method1("iscoroutinefunction", (&inner_test,))?
        .extract::<bool>()?;

    if !inner_is_async {
        return Ok(false);
    }

    // Replace inner_test with a sync wrapper that drives the coroutine.
    // Uses inline Python because PyCFunction closures lack the signature metadata and
    // calling conventions that Hypothesis requires to introspect and invoke inner_test.
    let sync_wrapper = async_runtime_attr(py, "_make_sync")?.call1((inner_test,))?;
    hypothesis_attr.setattr(py, "inner_test", sync_wrapper)?;

    Ok(true)
}

/// Sets `KARVA_ATTEMPT` and `KARVA_TOTAL_ATTEMPTS` on Python's `os.environ` so
/// the currently running test can read them.
pub fn set_attempt_env(py: Python<'_>, attempt: u32, total_attempts: u32) -> PyResult<()> {
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
pub fn set_test_name_env(py: Python<'_>, qualified_name: &str) -> PyResult<()> {
    let environ = py.import("os")?.getattr("environ")?;
    environ.set_item(WorkerEnvVars::KARVA_TEST_NAME, qualified_name)?;
    Ok(())
}

/// Formats Python values for test identity, quoting strings and escaping NUL bytes.
pub fn display_value(value: &Bound<'_, PyAny>) -> String {
    let display = if value.is_instance_of::<PyString>()
        && let Ok(repr) = value.repr()
    {
        repr.to_string()
    } else {
        value.to_string()
    };
    display.replace('\0', "\\x00")
}

fn truncated_display_value(value: &Bound<'_, PyAny>) -> String {
    let display = display_value(value);
    if display.chars().count() <= TRUNCATE_LENGTH {
        return display;
    }
    if value.is_instance_of::<PyString>()
        && let Ok(raw_value) = value.extract::<String>()
    {
        let mut truncated = raw_value.chars().take(TRUNCATE_LENGTH).collect::<String>();
        loop {
            let candidate = PyString::new(value.py(), &format!("{truncated}..."));
            if let Ok(repr) = candidate.repr()
                && repr.to_string().chars().count() <= TRUNCATE_LENGTH
            {
                return repr.to_string();
            }
            if truncated.pop().is_none() {
                break;
            }
        }
    }
    truncate_string(&display)
}

/// Adds a directory path to Python's sys.path at the specified index.
pub fn add_to_sys_path(py: Python<'_>, path: &Utf8Path, index: isize) -> PyResult<()> {
    let sys_module = py.import("sys")?;
    let sys_path = sys_module.getattr("path")?;
    sys_path.call_method1("insert", (index, path.to_string()))?;
    Ok(())
}

/// Renders parameter-list contents in Python signature order.
pub fn test_parameters(
    py: Python,
    kwargs: &FixtureArguments,
    parameters: &Parameters,
    name_only_arguments: &[&str],
) -> Option<String> {
    if kwargs.is_empty() {
        return None;
    }

    let mut rendered = String::new();
    for (index, (key, value)) in kwargs.iter_in_signature_order(parameters).enumerate() {
        if index > 0 {
            rendered.push_str(", ");
        }
        let truncated_key = truncate_string(key);
        if name_only_arguments.contains(&key.as_str()) {
            let _ = write!(rendered, "{truncated_key}");
        } else if let Ok(value) = value.cast_bound::<PyAny>(py) {
            let trimmed_value = truncated_display_value(value);
            let _ = write!(rendered, "{truncated_key}={trimmed_value}");
        }
    }
    Some(rendered)
}

/// Maximum display length for parameter keys and values in test names.
///
/// Keeps parameterized test names (e.g., `test_foo(key=value)`) readable in
/// CLI output by truncating long values with an ellipsis.
const TRUNCATE_LENGTH: usize = 30;

/// Truncates user-facing text by Unicode scalar count, preserving a three-character ellipsis.
pub fn truncate_string(value: &str) -> String {
    if value.chars().count() > TRUNCATE_LENGTH {
        let truncated: String = value.chars().take(TRUNCATE_LENGTH - 3).collect();
        format!("{truncated}...")
    } else {
        value.to_string()
    }
}
