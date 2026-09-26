use pyo3::exceptions::{PyOSError, PyRuntimeError};
use pyo3::prelude::*;

const STDIN_CAPTURE_ERROR: &str = "stdin is unavailable while test output is captured\n\nPass input explicitly to the code under test, or use --no-capture for an intentional interactive debugging session.";

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
        Err(PyRuntimeError::new_err(STDIN_CAPTURE_ERROR))
    }

    #[pyo3(signature = (_size = None))]
    fn read(&self, _size: Option<isize>) -> PyResult<()> {
        Err(PyRuntimeError::new_err(STDIN_CAPTURE_ERROR))
    }

    #[pyo3(signature = (_size = None))]
    fn readline(&self, _size: Option<isize>) -> PyResult<()> {
        Err(PyRuntimeError::new_err(STDIN_CAPTURE_ERROR))
    }

    #[pyo3(signature = (_hint = None))]
    fn readlines(&self, _hint: Option<isize>) -> PyResult<()> {
        Err(PyRuntimeError::new_err(STDIN_CAPTURE_ERROR))
    }

    fn readinto(&self, _buffer: &Bound<'_, PyAny>) -> PyResult<()> {
        Err(PyRuntimeError::new_err(STDIN_CAPTURE_ERROR))
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
/// `start` replaces `sys.stdout` and `sys.stderr` with `StringIO` objects,
/// replaces `sys.stdin` with [`CapturedStdin`], and redirects file descriptor
/// 0 to an EOF source. Callers must consume the value with [`Self::finish`] to
/// restore all streams and the descriptor.
pub struct PythonOutputCapture {
    sys: Py<PyModule>,
    os: Py<PyModule>,
    old_stdout: Py<PyAny>,
    old_stderr: Py<PyAny>,
    old_stdin: Py<PyAny>,
    old_stdin_fd: Option<i32>,
    stdout: Py<PyAny>,
    stderr: Py<PyAny>,
}

impl PythonOutputCapture {
    /// Redirects both Python output streams, rolling back stdout if stderr setup fails.
    pub fn start(py: Python<'_>) -> PyResult<Self> {
        let sys = py.import("sys")?.unbind();
        let os = py.import("os")?.unbind();
        let sys_bound = sys.bind(py);
        let os_bound = os.bind(py);
        let string_io = py.import("io")?.getattr("StringIO")?;

        let old_stdout = sys_bound.getattr("stdout")?.unbind();
        let old_stderr = sys_bound.getattr("stderr")?.unbind();
        let old_stdin = sys_bound.getattr("stdin")?.unbind();
        let stdout = string_io.call0()?.unbind();
        let stderr = string_io.call0()?.unbind();
        let captured_stdin = Py::new(py, CapturedStdin)?.into_any();

        sys_bound.setattr("stdout", stdout.bind(py))?;
        if let Err(err) = sys_bound.setattr("stderr", stderr.bind(py)) {
            if let Err(restore_err) = sys_bound.setattr("stdout", old_stdout.bind(py)) {
                tracing::warn!(
                    "failed to restore Python stdout after capture setup error: {restore_err}"
                );
            }
            return Err(err);
        }
        if let Err(err) = sys_bound.setattr("stdin", captured_stdin.bind(py)) {
            restore_stdio(sys_bound, &old_stdout, &old_stderr, py)?;
            return Err(err);
        }

        let old_stdin_fd = match redirect_stdin_fd(os_bound) {
            Ok(fd) => fd,
            Err(err) => {
                restore_stdio(sys_bound, &old_stdout, &old_stderr, py)?;
                sys_bound.setattr("stdin", old_stdin.bind(py))?;
                return Err(err);
            }
        };

        Ok(Self {
            sys,
            os,
            old_stdout,
            old_stderr,
            old_stdin,
            old_stdin_fd,
            stdout,
            stderr,
        })
    }

    /// Flushes captured streams, restores their original objects, and returns captured text.
    pub fn finish(self, py: Python<'_>) -> PyResult<CapturedPythonOutput> {
        let sys = self.sys.bind(py);
        let os = self.os.bind(py);
        let fd_result = restore_stdin_fd(os, self.old_stdin_fd);
        flush_current_streams(sys);

        let restore_result = restore_stdio(sys, &self.old_stdout, &self.old_stderr, py);
        let stdin_result = sys.setattr("stdin", self.old_stdin.bind(py));
        let stdout = self.stdout.bind(py).call_method0("getvalue")?.extract()?;
        let stderr = self.stderr.bind(py).call_method0("getvalue")?.extract()?;
        restore_result?;
        stdin_result?;
        fd_result?;

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

fn redirect_stdin_fd(os: &Bound<'_, PyModule>) -> PyResult<Option<i32>> {
    let old_fd = match os
        .getattr("dup")?
        .call1((0,))
        .and_then(|value| value.extract::<i32>())
    {
        Ok(fd) => Some(fd),
        Err(error) if is_bad_fd(os, &error)? => None,
        Err(error) => return Err(error),
    };
    let null_fd = match os
        .getattr("open")?
        .call1((os.getattr("devnull")?, os.getattr("O_RDONLY")?))?
        .extract::<i32>()
    {
        Ok(fd) => fd,
        Err(err) => {
            if let Some(old_fd) = old_fd {
                let _ = os.getattr("close")?.call1((old_fd,));
            }
            return Err(err);
        }
    };
    if let Err(err) = os.getattr("dup2")?.call1((null_fd, 0)) {
        let _ = os.getattr("close")?.call1((null_fd,));
        if let Some(old_fd) = old_fd {
            let _ = os.getattr("close")?.call1((old_fd,));
        }
        return Err(err);
    }
    if null_fd != 0 {
        if let Err(error) = os.getattr("close")?.call1((null_fd,)) {
            return match restore_stdin_fd(os, old_fd) {
                Ok(()) => Err(error),
                Err(restore_error) => Err(restore_error),
            };
        }
    }
    Ok(old_fd)
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
