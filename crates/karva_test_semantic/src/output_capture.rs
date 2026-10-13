//! Worker-side descriptor restoration and controller-owned output windows.
//!
//! Python owns rollback; private inherited handles delimit native bytes, allowing
//! the deadline watchdog to collect output without acquiring Python's GIL.

use std::fs::File;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::fd::{AsFd, AsRawFd};
#[cfg(windows)]
use std::os::windows::io::{AsHandle, AsRawHandle};
use std::sync::{Arc, Mutex};

use karva_diagnostic::{CapturedOutputReader, CapturedTestOutput};
use pyo3::exceptions::{PyOSError, PyRuntimeError};
use pyo3::prelude::*;

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

/// Per-attempt descriptor capture with bounded native buffers.
///
/// Callers must finish the capture before another attempt can redirect descriptors.
pub struct PythonOutputCapture {
    restoration: Py<PyAny>,
    native: Arc<NativeWindow>,
}

/// Owned duplicates allow the deadline watchdog to seal capture without the GIL.
struct NativeWindow {
    stdout: File,
    stderr: File,
    source: Arc<dyn CapturedOutputReader>,

    /// Serializes watchdog and test-thread sealing; the end marker is written once.
    output: Mutex<Option<CapturedTestOutput>>,
}

impl NativeWindow {
    fn boundary(&self, tag: u8) -> io::Result<()> {
        let mut marker = vec![0];
        marker.extend_from_slice(&self.source.marker());
        marker.push(tag);
        (&self.stdout).write_all(&marker)?;
        (&self.stderr).write_all(&marker)
    }

    fn collect(&self) -> Result<CapturedTestOutput, String> {
        let mut output = self
            .output
            .lock()
            .map_err(|_| "output capture lock poisoned")?;
        if let Some(output) = output.as_ref() {
            return Ok(output.clone());
        }
        self.boundary(b'E').map_err(|error| error.to_string())?;
        let collected = self.source.read()?;
        *output = Some(collected.clone());
        Ok(collected)
    }
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
    /// Opens an ordered window in the controller's native pipe readers.
    pub fn start(py: Python<'_>, source: Arc<dyn CapturedOutputReader>) -> PyResult<Self> {
        #[cfg(unix)]
        let (stdout, stderr) = (
            File::from(io::stdout().as_fd().try_clone_to_owned()?),
            File::from(io::stderr().as_fd().try_clone_to_owned()?),
        );
        #[cfg(windows)]
        let (stdout, stderr) = (
            File::from(io::stdout().as_handle().try_clone_to_owned()?),
            File::from(io::stderr().as_handle().try_clone_to_owned()?),
        );
        let native = Arc::new(NativeWindow {
            stdout,
            stderr,
            source,
            output: Mutex::new(None),
        });
        native.boundary(b'B')?;
        #[cfg(unix)]
        let handles = (native.stdout.as_raw_fd(), native.stderr.as_raw_fd());
        #[cfg(windows)]
        let handles = (
            native.stdout.as_raw_handle() as usize,
            native.stderr.as_raw_handle() as usize,
        );
        let restoration = match py
            .import("karva._output_capture")
            .and_then(|module| module.call_method1("redirect_output", handles))
        {
            Ok(value) => value.unbind(),
            Err(error) => {
                let _ = native.collect();
                return Err(error);
            }
        };
        Ok(Self {
            restoration,
            native,
        })
    }

    /// Shares native collection with the deadline watchdog.
    pub(super) fn watchdog_output(&self) -> SharedCapturedOutput {
        SharedCapturedOutput(Arc::clone(&self.native))
    }

    /// Restores descriptors and collects bytes preceding the ordered end markers.
    pub fn finish(self, py: Python<'_>) -> PyResult<CapturedPythonOutput> {
        flush_current_streams(&py.import("sys")?);
        let restore = self.restoration.bind(py).call_method0("close");
        let output = self.native.collect().map_err(PyRuntimeError::new_err);
        restore?;
        let output = output?;
        Ok(CapturedPythonOutput {
            stdout: output.stdout().to_owned(),
            stderr: output.stderr().to_owned(),
        })
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

/// Controller-owned output available to native deadline handling without Python.
#[derive(Clone)]
pub struct SharedCapturedOutput(Arc<NativeWindow>);

impl SharedCapturedOutput {
    pub(super) fn snapshot(&self) -> Option<CapturedTestOutput> {
        let output = self.0.collect().ok()?;
        (!output.is_empty()).then_some(output)
    }
}
