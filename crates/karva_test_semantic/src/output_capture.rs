use pyo3::prelude::*;

/// Process-global Python stream redirection for one test attempt.
///
/// `start` replaces `sys.stdout` and `sys.stderr` with `StringIO` objects;
/// callers must consume the value with [`Self::finish`] to restore both streams.
pub struct PythonOutputCapture {
    old_stdout: Py<PyAny>,
    old_stderr: Py<PyAny>,
    stdout: Py<PyAny>,
    stderr: Py<PyAny>,
}

impl PythonOutputCapture {
    /// Redirects both Python output streams, rolling back stdout if stderr setup fails.
    pub fn start(py: Python<'_>) -> PyResult<Self> {
        let sys = py.import("sys")?;
        let string_io = py.import("io")?.getattr("StringIO")?;

        let old_stdout = sys.getattr("stdout")?.unbind();
        let old_stderr = sys.getattr("stderr")?.unbind();
        let stdout = string_io.call0()?.unbind();
        let stderr = string_io.call0()?.unbind();

        sys.setattr("stdout", stdout.bind(py))?;
        if let Err(err) = sys.setattr("stderr", stderr.bind(py)) {
            if let Err(restore_err) = sys.setattr("stdout", old_stdout.bind(py)) {
                tracing::warn!(
                    "failed to restore Python stdout after capture setup error: {restore_err}"
                );
            }
            return Err(err);
        }

        Ok(Self {
            old_stdout,
            old_stderr,
            stdout,
            stderr,
        })
    }

    /// Flushes captured streams, restores their original objects, and returns captured text.
    pub fn finish(self, py: Python<'_>) -> PyResult<CapturedPythonOutput> {
        let sys = py.import("sys")?;
        flush_current_streams(&sys);

        let restore_result = restore_stdio(&sys, &self.old_stdout, &self.old_stderr, py);
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
