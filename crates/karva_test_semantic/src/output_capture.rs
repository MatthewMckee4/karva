use std::sync::{Arc, Mutex};

use karva_diagnostic::CapturedTestOutput;
use pyo3::exceptions::{PyOSError, PyRuntimeError};
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyCFunction, PyDict, PyString, PyTuple};

const STDIN_CAPTURE_ERROR: &str = "stdin is unavailable while test output is captured";

pyo3::create_exception!(karva, CapturedStdinError, pyo3::exceptions::PyRuntimeError);

/// Python-facing stdin replacement that fails reads instead of waiting on a terminal.
#[pyclass]
struct CapturedStdin;

#[pymethods]
#[expect(clippy::unused_self)]
impl CapturedStdin {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&self) -> PyResult<()> {
        Err(CapturedStdinError::new_err(STDIN_CAPTURE_ERROR))
    }

    #[pyo3(signature = (_size = None))]
    fn read(&self, _size: Option<isize>) -> PyResult<()> {
        Err(CapturedStdinError::new_err(STDIN_CAPTURE_ERROR))
    }

    #[pyo3(signature = (_size = None))]
    fn readline(&self, _size: Option<isize>) -> PyResult<()> {
        Err(CapturedStdinError::new_err(STDIN_CAPTURE_ERROR))
    }

    #[pyo3(signature = (_hint = None))]
    fn readlines(&self, _hint: Option<isize>) -> PyResult<()> {
        Err(CapturedStdinError::new_err(STDIN_CAPTURE_ERROR))
    }

    fn readinto(&self, _buffer: &Bound<'_, PyAny>) -> PyResult<()> {
        Err(CapturedStdinError::new_err(STDIN_CAPTURE_ERROR))
    }

    fn readable(&self) -> bool {
        false
    }

    fn isatty(&self) -> bool {
        false
    }

    fn fileno(&self) -> u8 {
        0
    }

    #[getter]
    fn buffer(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(Py::new(py, Self)?.into_any())
    }
}

/// Process-global Python stream redirection for one test attempt.
///
/// `start` replaces `sys.stdout` and `sys.stderr` with `StringIO` objects.
/// Callers must consume the value with [`Self::finish`] to restore both streams.
pub struct PythonOutputCapture {
    sys: Py<PyModule>,
    old_stdout: Py<PyAny>,
    old_stderr: Py<PyAny>,
    stdout: Py<PyAny>,
    stderr: Py<PyAny>,
}

/// Keeps Python's stdin guard installed for a worker's captured tests.
pub struct PythonStdinCapture {
    sys: Py<PyModule>,
    old_stdin: Py<PyAny>,
    captured_stdin: Py<PyAny>,
}

impl PythonStdinCapture {
    /// Installs the captured stdin guard and saves the original stream.
    pub fn start(py: Python<'_>) -> PyResult<Self> {
        let sys = py.import("sys")?.unbind();
        let sys_bound = sys.bind(py);
        let old_stdin = sys_bound.getattr("stdin")?.unbind();
        let captured_stdin = Py::new(py, CapturedStdin)?.into_any();
        sys_bound.setattr("stdin", captured_stdin.bind(py))?;
        Ok(Self {
            sys,
            old_stdin,
            captured_stdin,
        })
    }

    /// Reinstalls the guard after test code changes `sys.stdin`.
    pub fn activate(&self, py: Python<'_>) -> PyResult<()> {
        self.sys
            .bind(py)
            .setattr("stdin", self.captured_stdin.bind(py))
    }

    /// Restores the worker's original Python stdin stream.
    pub fn finish(self, py: Python<'_>) -> PyResult<()> {
        self.sys.bind(py).setattr("stdin", self.old_stdin.bind(py))
    }
}

/// Keeps process stdin at EOF while captured tests execute.
pub struct StdinCapture {
    os: Py<PyModule>,
    dup2: Py<PyAny>,
    old_fd: Option<i32>,
    null_fd: i32,
}

impl StdinCapture {
    /// Saves file descriptor 0 and replaces it with a persistent `/dev/null` handle.
    pub fn start(py: Python<'_>) -> PyResult<Self> {
        let os = py.import("os")?.unbind();
        let os_bound = os.bind(py);
        let dup2 = os_bound.getattr("dup2")?.unbind();
        let old_fd = match os_bound
            .getattr("dup")?
            .call1((0,))
            .and_then(|value| value.extract::<i32>())
        {
            Ok(fd) => Some(fd),
            Err(error) if is_bad_fd(os_bound, &error)? => None,
            Err(error) => return Err(error),
        };
        let null_fd = match os_bound
            .getattr("open")?
            .call1((os_bound.getattr("devnull")?, os_bound.getattr("O_RDONLY")?))?
            .extract::<i32>()
        {
            Ok(fd) => fd,
            Err(err) => {
                if let Some(old_fd) = old_fd {
                    let _ = os_bound.getattr("close")?.call1((old_fd,));
                }
                return Err(err);
            }
        };
        if let Err(err) = dup2.bind(py).call1((null_fd, 0)) {
            let _ = os_bound.getattr("close")?.call1((null_fd,));
            if let Some(old_fd) = old_fd {
                let _ = os_bound.getattr("close")?.call1((old_fd,));
            }
            return Err(err);
        }

        Ok(Self {
            os,
            dup2,
            old_fd,
            null_fd,
        })
    }

    /// Reapplies the EOF source if test code changed file descriptor 0.
    pub fn activate(&self, py: Python<'_>) -> PyResult<()> {
        self.dup2.bind(py).call1((self.null_fd, 0))?;
        Ok(())
    }

    /// Restores the inherited descriptor and closes the persistent EOF handle.
    pub fn finish(self, py: Python<'_>) -> PyResult<()> {
        let os = self.os.bind(py);
        let restore_result = restore_stdin_fd(os, self.old_fd);
        let close_result = if self.null_fd == 0 && self.old_fd.is_none() {
            Ok(())
        } else {
            os.getattr("close")?.call1((self.null_fd,)).map(|_| ())
        };
        restore_result.and(close_result)
    }
}

impl PythonOutputCapture {
    /// Redirects both Python output streams, rolling back stdout if stderr setup fails.
    pub fn start(py: Python<'_>) -> PyResult<Self> {
        let sys = py.import("sys")?.unbind();
        let sys_bound = sys.bind(py);
        let string_io = py.import("io")?.getattr("StringIO")?;

        let old_stdout = sys_bound.getattr("stdout")?.unbind();
        let old_stderr = sys_bound.getattr("stderr")?.unbind();
        let stdout = string_io.call0()?.unbind();
        let stderr = string_io.call0()?.unbind();

        sys_bound.setattr("stdout", stdout.bind(py))?;
        if let Err(err) = sys_bound.setattr("stderr", stderr.bind(py)) {
            if let Err(restore_err) = sys_bound.setattr("stdout", old_stdout.bind(py)) {
                tracing::warn!(
                    "failed to restore Python stdout after capture setup error: {restore_err}"
                );
            }
            return Err(err);
        }
        Ok(Self {
            sys,
            old_stdout,
            old_stderr,
            stdout,
            stderr,
        })
    }

    /// Mirrors timed synchronous output in Rust so a watchdog can read it without the GIL.
    /// Ordinary captures retain the existing `StringIO` path and its allocation behavior.
    pub(super) fn watchdog_output(&mut self, py: Python<'_>) -> PyResult<SharedCapturedOutput> {
        let sys = self.sys.bind(py);
        let stdout = mirror_stream(py, sys, "stdout", &mut self.stdout)?;
        let stderr = mirror_stream(py, sys, "stderr", &mut self.stderr)?;
        Ok(SharedCapturedOutput { stdout, stderr })
    }

    /// Flushes captured streams, restores their original objects, and returns captured text.
    pub fn finish(self, py: Python<'_>) -> PyResult<CapturedPythonOutput> {
        let sys = self.sys.bind(py);
        flush_current_streams(sys);

        let restore_result = restore_stdio(sys, &self.old_stdout, &self.old_stderr, py);
        let stdout = self.stdout.bind(py).call_method0("getvalue")?.extract()?;
        let stderr = self.stderr.bind(py).call_method0("getvalue")?.extract()?;
        restore_result?;

        Ok(CapturedPythonOutput { stdout, stderr })
    }
}

/// Text emitted through Python's stdout and stderr during one capture window.
pub struct CapturedPythonOutput {
    /// Captured stdout, preserving write order within that stream.
    pub stdout: String,

    /// Captured stderr, preserving write order within that stream.
    pub stderr: String,
}

fn flush_current_streams(sys: &Bound<'_, PyModule>) {
    for stream in ["stdout", "stderr"] {
        if let Err(err) = sys
            .getattr(stream)
            .and_then(|stream| stream.call_method0("flush"))
        {
            tracing::warn!("failed to flush captured Python {stream}: {err}");
        }
    }
}

fn restore_stdio(
    sys: &Bound<'_, PyModule>,
    stdout: &Py<PyAny>,
    stderr: &Py<PyAny>,
    py: Python<'_>,
) -> PyResult<()> {
    sys.setattr("stdout", stdout.bind(py))?;
    sys.setattr("stderr", stderr.bind(py))
}

fn restore_stdin_fd(os: &Bound<'_, PyModule>, old_fd: Option<i32>) -> PyResult<()> {
    let result = match old_fd {
        Some(old_fd) => os.getattr("dup2")?.call1((old_fd, 0)),
        None => os.getattr("close")?.call1((0,)),
    };
    if let Some(old_fd) = old_fd {
        let close_result = os.getattr("close")?.call1((old_fd,));
        result.and(close_result).map(|_| ())
    } else {
        match result {
            Ok(_) => Ok(()),
            Err(error) if is_bad_fd(os, &error)? => Ok(()),
            Err(error) => Err(error),
        }
    }
}

fn is_bad_fd(os: &Bound<'_, PyModule>, error: &PyErr) -> PyResult<bool> {
    let py = os.py();
    if !error.is_instance_of::<PyOSError>(py) {
        return Ok(false);
    }
    let bad_fd = py.import("errno")?.getattr("EBADF")?.extract::<i32>()?;
    Ok(error.value(py).getattr("errno")?.extract::<i32>()? == bad_fd)
}

/// Captured text available to native deadline handling without acquiring Python.
#[derive(Clone)]
pub struct SharedCapturedOutput {
    stdout: Arc<Mutex<String>>,
    stderr: Arc<Mutex<String>>,
}

impl SharedCapturedOutput {
    pub(super) fn snapshot(&self) -> Option<CapturedTestOutput> {
        let stdout = self.stdout.lock().ok()?.clone();
        let stderr = self.stderr.lock().ok()?.clone();
        let output = CapturedTestOutput::new(stdout, stderr);
        (!output.is_empty()).then_some(output)
    }
}

/// `StringIO` semantics are preserved; only timed calls pay for a native text mirror.
fn mirror_stream(
    py: Python<'_>,
    sys: &Bound<'_, PyModule>,
    name: &str,
    stream: &mut Py<PyAny>,
) -> PyResult<Arc<Mutex<String>>> {
    static CLASS: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
    let class = CLASS.get_or_try_init(py, || {
        let code = cr"
import io

class TimeoutCapture(io.StringIO):
    def __init__(self, initial, record):
        super().__init__(initial)
        self.seek(0, 2)
        self._length = len(initial)
        self._record = record

    def write(self, text):
        append = self.tell() == self._length
        written = super().write(text)
        self._length = max(self._length, self.tell())
        self._record(text if append else self.getvalue(), not append)
        return written

    def truncate(self, size=None):
        result = super().truncate(size)
        self._length = len(self.getvalue())
        self._record(self.getvalue(), True)
        return result
";
        let module = PyModule::from_code(
            py,
            code,
            c"karva_timeout_capture.py",
            c"karva_timeout_capture",
        )?;
        Ok::<_, PyErr>(module.getattr("TimeoutCapture")?.unbind())
    })?;
    let initial = stream
        .bind(py)
        .call_method0("getvalue")?
        .extract::<String>()?;
    let shared = Arc::new(Mutex::new(initial.clone()));
    let captured = Arc::clone(&shared);
    let callback = PyCFunction::new_closure(
        py,
        None,
        None,
        move |args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>| -> PyResult<()> {
            let text = args.get_item(0)?.cast_into::<PyString>()?;
            let text = text.to_string_lossy();
            let replace = args.get_item(1)?.extract::<bool>()?;
            let mut captured = captured
                .lock()
                .map_err(|_| PyRuntimeError::new_err("timeout output capture lock poisoned"))?;
            if replace {
                captured.clear();
            }
            captured.push_str(&text);
            Ok(())
        },
    )?;
    let mirrored = class.bind(py).call1((initial, callback))?.unbind();
    if sys.getattr(name)?.is(stream.bind(py)) {
        sys.setattr(name, mirrored.bind(py))?;
    }
    *stream = mirrored;
    Ok(shared)
}
