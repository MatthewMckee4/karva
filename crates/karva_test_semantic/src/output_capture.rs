use pyo3::prelude::*;

pub struct PythonOutputCapture {
    old_stdout: Py<PyAny>,
    old_stderr: Py<PyAny>,
    stdout: Py<PyAny>,
    stderr: Py<PyAny>,
    file_descriptors: FileDescriptorCapture,
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

        let file_descriptors = match FileDescriptorCapture::start(py) {
            Ok(capture) => capture,
            Err(err) => {
                if let Err(restore_err) = restore_stdio(&sys, &old_stdout, &old_stderr, py) {
                    tracing::warn!(
                        "failed to restore Python output after file descriptor capture setup error: {restore_err}"
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
            file_descriptors,
        })
    }

    pub fn finish(self, py: Python<'_>) -> PyResult<CapturedPythonOutput> {
        let sys = py.import("sys")?;
        flush_current_streams(&sys);

        let restore_result = restore_stdio(&sys, &self.old_stdout, &self.old_stderr, py);
        let stdout_result = self.stdout.bind(py).call_method0("getvalue")?.extract();
        let stderr_result = self.stderr.bind(py).call_method0("getvalue")?.extract();
        let file_descriptor_result = self.file_descriptors.finish(py);
        restore_result?;
        let mut stdout: String = stdout_result?;
        let mut stderr: String = stderr_result?;
        let file_descriptor_output = file_descriptor_result?;
        stdout.push_str(&file_descriptor_output.stdout);
        stderr.push_str(&file_descriptor_output.stderr);

        Ok(CapturedPythonOutput { stdout, stderr })
    }

    pub fn with_file_descriptors_restored<R>(&self, py: Python<'_>, f: impl FnOnce() -> R) -> R {
        if let Err(err) = self.file_descriptors.suspend(py) {
            tracing::warn!("failed to suspend file descriptor output capture: {err}");
        }
        let result = f();
        if let Err(err) = self.file_descriptors.resume(py) {
            tracing::warn!("failed to resume file descriptor output capture: {err}");
        }
        result
    }
}

pub struct CapturedPythonOutput {
    pub stdout: String,
    pub stderr: String,
}

struct FileDescriptorCapture {
    stdout: Py<PyAny>,
    stderr: Py<PyAny>,
    old_stdout: i32,
    old_stderr: i32,
}

impl FileDescriptorCapture {
    fn start(py: Python<'_>) -> PyResult<Self> {
        let temporary_file = py.import("tempfile")?.getattr("TemporaryFile")?;
        let stdout = temporary_file.call1(("w+b",))?.unbind();
        let stderr = temporary_file.call1(("w+b",))?.unbind();
        let os = py.import("os")?;
        let old_stdout = os.call_method1("dup", (1,))?.extract()?;
        let old_stderr = match os.call_method1("dup", (2,)).and_then(|fd| fd.extract()) {
            Ok(fd) => fd,
            Err(err) => {
                if let Err(close_err) = os.call_method1("close", (old_stdout,)) {
                    tracing::warn!(
                        "failed to close saved stdout after capture setup error: {close_err}"
                    );
                }
                return Err(err);
            }
        };
        let capture = Self {
            stdout,
            stderr,
            old_stdout,
            old_stderr,
        };
        if let Err(err) = capture.resume(py) {
            if let Err(close_err) = capture.close_resources(py) {
                tracing::warn!(
                    "failed to close file descriptor capture after setup error: {close_err}"
                );
            }
            return Err(err);
        }
        Ok(capture)
    }

    fn finish(self, py: Python<'_>) -> PyResult<CapturedPythonOutput> {
        let suspend_result = self.suspend(py);
        let stdout_result = read_file(py, &self.stdout);
        let stderr_result = read_file(py, &self.stderr);
        let close_result = self.close_resources(py);
        suspend_result?;
        let stdout = stdout_result?;
        let stderr = stderr_result?;
        close_result?;
        Ok(CapturedPythonOutput { stdout, stderr })
    }

    fn suspend(&self, py: Python<'_>) -> PyResult<()> {
        let os = py.import("os")?;
        os.call_method1("dup2", (self.old_stdout, 1))?;
        if let Err(err) = os.call_method1("dup2", (self.old_stderr, 2)) {
            if let Err(restore_err) = redirect_descriptor(&os, py, &self.stdout, 1) {
                tracing::warn!("failed to restore stdout capture after setup error: {restore_err}");
            }
            return Err(err);
        }
        Ok(())
    }

    fn resume(&self, py: Python<'_>) -> PyResult<()> {
        let os = py.import("os")?;
        redirect_descriptor(&os, py, &self.stdout, 1)?;
        if let Err(err) = redirect_descriptor(&os, py, &self.stderr, 2) {
            if let Err(restore_err) = os.call_method1("dup2", (self.old_stdout, 1)) {
                tracing::warn!("failed to restore stdout after capture setup error: {restore_err}");
            }
            return Err(err);
        }
        Ok(())
    }

    fn close_resources(&self, py: Python<'_>) -> PyResult<()> {
        let os = py.import("os")?;
        let results = [
            self.stdout.bind(py).call_method0("close").map(|_| ()),
            self.stderr.bind(py).call_method0("close").map(|_| ()),
            os.call_method1("close", (self.old_stdout,)).map(|_| ()),
            os.call_method1("close", (self.old_stderr,)).map(|_| ()),
        ];
        for result in results {
            result?;
        }
        Ok(())
    }
}

fn redirect_descriptor(
    os: &Bound<'_, PyModule>,
    py: Python<'_>,
    file: &Py<PyAny>,
    target: i32,
) -> PyResult<()> {
    let source: i32 = file.bind(py).call_method0("fileno")?.extract()?;
    os.call_method1("dup2", (source, target))?;
    Ok(())
}

fn read_file(py: Python<'_>, file: &Py<PyAny>) -> PyResult<String> {
    let file = file.bind(py);
    file.call_method1("seek", (0,))?;
    file.call_method0("read")?
        .call_method1("decode", ("utf-8", "replace"))?
        .extract()
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
