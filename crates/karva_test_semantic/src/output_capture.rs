use pyo3::prelude::*;

pub struct PythonOutputCapture {
    old_stdout: Py<PyAny>,
    old_stderr: Py<PyAny>,
    stdout: Py<PyAny>,
    stderr: Py<PyAny>,
    stdin: StdinCapture,
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
        let stdin = match StdinCapture::start(py) {
            Ok(capture) => capture,
            Err(err) => {
                if let Err(restore_err) = restore_stdio(&sys, &old_stdout, &old_stderr, py) {
                    tracing::warn!(
                        "failed to restore Python output after stdin capture setup error: {restore_err}"
                    );
                }
                return Err(err);
            }
        };

        Ok(Self {
            old_stdout,
            old_stderr,
            stdout,
            stderr,
            stdin,
        })
    }

    pub fn finish(self, py: Python<'_>) -> PyResult<CapturedPythonOutput> {
        let sys = py.import("sys")?;
        flush_current_streams(&sys);

        let restore_result = restore_stdio(&sys, &self.old_stdout, &self.old_stderr, py);
        let stdin_result = self.stdin.finish(py);
        let stdout = self.stdout.bind(py).call_method0("getvalue")?.extract()?;
        let stderr = self.stderr.bind(py).call_method0("getvalue")?.extract()?;
        restore_result?;
        stdin_result?;

        Ok(CapturedPythonOutput { stdout, stderr })
    }
}

pub struct CapturedPythonOutput {
    pub stdout: String,
    pub stderr: String,
}

struct StdinCapture {
    old_stdin: Py<PyAny>,
    old_stdin_fd: i64,
}

impl StdinCapture {
    fn start(py: Python<'_>) -> PyResult<Self> {
        let os = py.import("os")?;
        let sys = py.import("sys")?;
        let builtins = py.import("builtins")?;
        let old_stdin = sys.getattr("stdin")?.unbind();
        let eof = builtins
            .getattr("open")?
            .call1((os.getattr("devnull")?, "rb"))?;
        let unreadable = py
            .import("karva._builtins")?
            .getattr("_CapturedStdin")?
            .call0()?;
        let eof_fd: i64 = eof.call_method0("fileno")?.extract()?;
        let old_stdin_fd = os.call_method1("dup", (0,))?.extract()?;
        let capture = Self {
            old_stdin,
            old_stdin_fd,
        };

        let start_result = os
            .call_method1("dup2", (eof_fd, 0))
            .and_then(|_| sys.setattr("stdin", unreadable));
        if let Err(err) = start_result {
            if let Err(restore_err) = capture.finish(py) {
                tracing::warn!("failed to restore stdin after capture setup error: {restore_err}");
            }
            return Err(err);
        }

        Ok(capture)
    }

    fn finish(self, py: Python<'_>) -> PyResult<()> {
        let os = py.import("os")?;
        let sys = py.import("sys")?;
        let descriptor_result = os.call_method1("dup2", (self.old_stdin_fd, 0)).map(|_| ());
        let stdin_result = sys.setattr("stdin", self.old_stdin.bind(py));
        let close_result = os.call_method1("close", (self.old_stdin_fd,)).map(|_| ());
        descriptor_result?;
        stdin_result?;
        close_result
    }
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
