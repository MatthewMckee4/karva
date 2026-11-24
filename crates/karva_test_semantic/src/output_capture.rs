use std::sync::{Arc, Mutex};

use karva_diagnostic::CapturedTestOutput;
use pyo3::exceptions::{PyOSError, PyRuntimeError};
use pyo3::intern;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyCFunction, PyDict, PyString, PyTuple};

const STDIN_CAPTURE_ERROR: &str = "stdin is unavailable while test output is captured";

pyo3::create_exception!(karva, CapturedStdinError, pyo3::exceptions::PyRuntimeError);

/// Python-facing stdin replacement that fails reads instead of waiting on a terminal.
///
/// Instance attributes let tests monkeypatch stream methods such as `isatty`.
#[pyclass(dict)]
struct CapturedStdin;

#[pymethods]
#[expect(clippy::unused_self)]
impl CapturedStdin {
    /// Allows multiprocessing fork bootstrap to close its inherited stream.
    /// The runner keeps owning descriptor zero and reinstalls this read guard.
    fn close(&self) {}

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
/// Captured streams allocate a temporary file only when a descriptor is requested.
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
            .setattr(intern!(py, "stdin"), self.captured_stdin.bind(py))
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
        let sys = py.import(intern!(py, "sys"))?.unbind();
        let sys_bound = sys.bind(py);
        let capture_stream = capture_stream_class(py)?.bind(py);

        let old_stdout = sys_bound.getattr(intern!(py, "stdout"))?.unbind();
        let old_stderr = sys_bound.getattr(intern!(py, "stderr"))?.unbind();
        let stdout = capture_stream.call0()?.unbind();
        let stderr = capture_stream.call0()?.unbind();

        sys_bound.setattr(intern!(py, "stdout"), stdout.bind(py))?;
        if let Err(err) = sys_bound.setattr(intern!(py, "stderr"), stderr.bind(py)) {
            if let Err(restore_err) = sys_bound.setattr(intern!(py, "stdout"), old_stdout.bind(py))
            {
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
    pub(super) fn watchdog_output(&self, py: Python<'_>) -> PyResult<SharedCapturedOutput> {
        let stdout = mirror_stream(self.stdout.bind(py))?;
        let stderr = mirror_stream(self.stderr.bind(py))?;
        Ok(SharedCapturedOutput { stdout, stderr })
    }

    /// Flushes captured streams, restores their original objects, and returns captured text.
    pub fn finish(self, py: Python<'_>) -> PyResult<CapturedPythonOutput> {
        let sys = self.sys.bind(py);
        flush_current_streams(sys);

        let restore_result = restore_stdio(sys, &self.old_stdout, &self.old_stderr, py);
        let stdout = self
            .stdout
            .bind(py)
            .call_method0(intern!(py, "getvalue"))?
            .extract()?;
        let stderr = self
            .stderr
            .bind(py)
            .call_method0(intern!(py, "getvalue"))?
            .extract()?;
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
    for stream in [intern!(sys.py(), "stdout"), intern!(sys.py(), "stderr")] {
        if let Err(err) = sys
            .getattr(stream)
            .and_then(|stream| stream.call_method0(intern!(sys.py(), "flush")))
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
    sys.setattr(intern!(py, "stdout"), stdout.bind(py))?;
    sys.setattr(intern!(py, "stderr"), stderr.bind(py))
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

fn capture_stream_class(py: Python<'_>) -> PyResult<&Py<PyAny>> {
    static CLASS: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
    CLASS.get_or_try_init(py, || {
        let code = cr"
import io
import os
import tempfile
from threading import RLock

class CapturedOutput(io.TextIOBase):
    def __init__(self):
        self._stream = io.StringIO()
        self._reader = None
        self._record = None
        self._lock = RLock()

    def fileno(self):
        with self._lock:
            if self._reader is None:
                self._rollover()
            return self._stream.fileno()

    def _rollover(self):
        text = self._stream.getvalue()
        position = self._stream.tell()
        delete_on_close = os.name == 'nt'
        output = tempfile.NamedTemporaryFile(buffering=0, delete=delete_on_close)
        try:
            try:
                # Open separately so snapshots cannot move an inherited writer's offset.
                reader = open(
                    output.name,
                    'rb',
                    buffering=0,
                    opener=lambda path, flags: os.open(path, flags | getattr(os, 'O_TEMPORARY', 0)),
                )
            finally:
                if not delete_on_close:
                    os.unlink(output.name)
        except BaseException:
            output.close()
            raise
        try:
            stream = io.TextIOWrapper(
                io.BufferedRandom(output), encoding='utf-8', errors='replace', newline='',
            )
            stream.write(text)
            stream.flush()
            end = stream.tell()
            if position != len(text):
                prefix = text[:position].encode('utf-8', 'replace')
                stream.seek(len(prefix) + max(0, position - len(text)))
        except BaseException:
            reader.close()
            output.close()
            raise
        self._stream = stream
        self._reader = reader
        self._end = end

    def __getattr__(self, name):
        return getattr(self._stream, name)

    @property
    def closed(self):
        return self._stream.closed

    @property
    def encoding(self):
        return self._stream.encoding

    @property
    def errors(self):
        return self._stream.errors

    @property
    def newlines(self):
        return self._stream.newlines

    def close(self):
        with self._lock:
            try:
                self._stream.close()
            finally:
                if self._reader is not None:
                    self._reader.close()

    def flush(self):
        with self._lock:
            self._stream.flush()

    def readable(self):
        return self._stream.readable()

    def writable(self):
        return self._stream.writable()

    def seekable(self):
        return self._stream.seekable()

    def seek(self, offset, whence=0):
        with self._lock:
            return self._stream.seek(offset, whence)

    def tell(self):
        with self._lock:
            return self._stream.tell()

    def read(self, size=-1):
        with self._lock:
            return self._stream.read(size)

    def readline(self, size=-1):
        with self._lock:
            return self._stream.readline(size)

    def getvalue(self):
        with self._lock:
            if self._reader is None:
                return self._stream.getvalue()
            self._stream.flush()
            self._reader.seek(0)
            return self._reader.read().decode('utf-8', 'replace')

    def mirror(self, record):
        with self._lock:
            self._record = record
            self._record_all()

    def _record_all(self):
        text = self.getvalue()
        self._end = len(text) if self._reader is None else self._reader.tell()
        self._record(text, True)

    def write(self, text):
        with self._lock:
            append = self._record is not None and self._stream.tell() == self._end
            written = self._stream.write(text)
            if self._reader is not None:
                # Flush before descriptor writes can overtake buffered text.
                self._stream.flush()
            if self._record is not None:
                if append:
                    self._end = self._stream.tell()
                    self._record(text, False)
                else:
                    self._record_all()
            return written

    def truncate(self, size=None):
        with self._lock:
            result = self._stream.truncate(size)
            if self._record is not None:
                self._record_all()
            return result
";
        let module = PyModule::from_code(
            py,
            code,
            c"karva_output_capture.py",
            c"karva_output_capture",
        )?;
        Ok(module.getattr("CapturedOutput")?.unbind())
    })
}

fn mirror_stream(stream: &Bound<'_, PyAny>) -> PyResult<Arc<Mutex<String>>> {
    let py = stream.py();
    let shared = Arc::new(Mutex::new(String::new()));
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
    stream.call_method1(intern!(py, "mirror"), (callback,))?;
    Ok(shared)
}
