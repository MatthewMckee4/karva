use pyo3::prelude::*;

use karva_logging::{SavedStdout, save_stdout};

pub struct OutputCapture {
    file_descriptors: FileDescriptorCapture,
    _saved_stdout: SavedStdout,
}

impl OutputCapture {
    pub fn new(py: Python<'_>) -> PyResult<Self> {
        let stdout = save_stdout()?;
        let file_descriptors = FileDescriptorCapture::new(py)?;
        file_descriptors.resume(py)?;
        Ok(Self {
            file_descriptors,
            _saved_stdout: stdout,
        })
    }

    pub fn start(&self, py: Python<'_>) -> PyResult<PythonOutputCapture<'_>> {
        PythonOutputCapture::start(py, &self.file_descriptors)
    }

    pub fn stop(&self, py: Python<'_>) -> PyResult<()> {
        self.file_descriptors.suspend(py)
    }
}

pub struct PythonOutputCapture<'capture> {
    old_stdout: Py<PyAny>,
    old_stderr: Py<PyAny>,
    stdout: Py<PyAny>,
    stderr: Py<PyAny>,
    file_descriptors: &'capture FileDescriptorCapture,
}

impl<'capture> PythonOutputCapture<'capture> {
    fn start(py: Python<'_>, file_descriptors: &'capture FileDescriptorCapture) -> PyResult<Self> {
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

        if let Err(err) = file_descriptors.start(py) {
            if let Err(restore_err) = restore_stdio(&sys, &old_stdout, &old_stderr, py) {
                tracing::warn!(
                    "failed to restore Python output after file descriptor capture setup error: {restore_err}"
                );
            }
            return Err(err);
        }

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
}

pub fn with_restored_file_descriptors<R>(
    capture: Option<&PythonOutputCapture<'_>>,
    py: Python<'_>,
    f: impl FnOnce() -> R,
) -> R {
    let Some(capture) = capture else {
        return f();
    };
    let current_file_descriptors = match SavedFileDescriptors::new(py) {
        Ok(file_descriptors) => file_descriptors,
        Err(err) => {
            tracing::warn!("failed to save active file descriptors: {err}");
            return f();
        }
    };
    if let Err(err) = capture.file_descriptors.suspend(py) {
        tracing::warn!("failed to suspend file descriptor output capture: {err}");
        if let Err(restore_err) = current_file_descriptors.restore(py) {
            tracing::warn!("failed to restore active file descriptors: {restore_err}");
        }
        return f();
    }
    let result = f();
    if let Err(err) = current_file_descriptors.restore(py) {
        tracing::warn!("failed to restore active file descriptors: {err}");
    }
    result
}

pub struct CapturedPythonOutput {
    pub stdout: String,
    pub stderr: String,
}

struct FileDescriptorCapture {
    stdout: Py<PyAny>,
    stderr: Py<PyAny>,
    old: SavedFileDescriptors,
}

impl FileDescriptorCapture {
    fn new(py: Python<'_>) -> PyResult<Self> {
        let temporary_file = py.import("tempfile")?.getattr("TemporaryFile")?;
        let stdout = temporary_file.call1(("w+b",))?.unbind();
        let stderr = temporary_file.call1(("w+b",))?.unbind();
        let old = SavedFileDescriptors::new(py)?;
        Ok(Self {
            stdout,
            stderr,
            old,
        })
    }

    fn start(&self, py: Python<'_>) -> PyResult<()> {
        clear_if_needed(py, &self.stdout)?;
        clear_if_needed(py, &self.stderr)
    }

    fn finish(&self, py: Python<'_>) -> PyResult<CapturedPythonOutput> {
        let stdout_result = read_file(py, &self.stdout);
        let stderr_result = read_file(py, &self.stderr);
        let stdout = stdout_result?;
        let stderr = stderr_result?;
        Ok(CapturedPythonOutput { stdout, stderr })
    }

    fn suspend(&self, py: Python<'_>) -> PyResult<()> {
        if let Err(err) = self.old.restore(py) {
            let os = py.import("os")?;
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
            if let Err(restore_err) = redirect_descriptor(&os, py, &self.old.stdout, 1) {
                tracing::warn!("failed to restore stdout after capture setup error: {restore_err}");
            }
            return Err(err);
        }
        Ok(())
    }
}

struct SavedFileDescriptors {
    stdout: Py<PyAny>,
    stderr: Py<PyAny>,
}

impl SavedFileDescriptors {
    fn new(py: Python<'_>) -> PyResult<Self> {
        let os = py.import("os")?;
        Ok(Self {
            stdout: duplicate_descriptor(&os, 1)?,
            stderr: duplicate_descriptor(&os, 2)?,
        })
    }

    fn restore(&self, py: Python<'_>) -> PyResult<()> {
        let os = py.import("os")?;
        redirect_descriptor(&os, py, &self.stdout, 1)?;
        redirect_descriptor(&os, py, &self.stderr, 2)
    }
}

fn duplicate_descriptor(os: &Bound<'_, PyModule>, target: i32) -> PyResult<Py<PyAny>> {
    let descriptor: i32 = os.call_method1("dup", (target,))?.extract()?;
    match os.call_method1("fdopen", (descriptor, "wb")) {
        Ok(file) => Ok(file.unbind()),
        Err(err) => {
            if let Err(close_err) = os.call_method1("close", (descriptor,)) {
                tracing::warn!("failed to close duplicated file descriptor: {close_err}");
            }
            Err(err)
        }
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

fn file_position(py: Python<'_>, file: &Py<PyAny>) -> PyResult<u64> {
    file.bind(py).call_method0("tell")?.extract()
}

fn clear_if_needed(py: Python<'_>, file: &Py<PyAny>) -> PyResult<()> {
    let position = file_position(py, file)?;
    if position == 0 {
        return Ok(());
    }
    let file = file.bind(py);
    file.call_method1("seek", (0,))?;
    file.call_method1("truncate", (0,))?;
    Ok(())
}

fn read_file(py: Python<'_>, file: &Py<PyAny>) -> PyResult<String> {
    let file = file.bind(py);
    let end = file.call_method0("tell")?.extract::<u64>()?;
    if end == 0 {
        return Ok(String::new());
    }
    file.call_method1("seek", (0,))?;
    let output = file
        .call_method1("read", (end,))?
        .call_method1("decode", ("utf-8", "replace"))?
        .extract();
    file.call_method1("seek", (0,))?;
    file.call_method1("truncate", (0,))?;
    output
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
