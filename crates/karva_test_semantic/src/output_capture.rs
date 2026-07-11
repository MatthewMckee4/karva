use pyo3::prelude::*;

pub struct PythonOutputCapture {
    old_stdout: Py<PyAny>,
    old_stderr: Py<PyAny>,
    stdout: Py<PyAny>,
    stderr: Py<PyAny>,
}

impl PythonOutputCapture {
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

pub struct CapturedPythonOutput {
    pub stdout: String,
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
