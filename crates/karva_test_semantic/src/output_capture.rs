#[cfg(any(unix, windows))]
use std::collections::VecDeque;
#[cfg(any(unix, windows))]
use std::io::{self, Read};
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(any(unix, windows))]
use std::sync::{Arc, Mutex};
#[cfg(unix)]
use std::sync::{Condvar, OnceLock, mpsc};
#[cfg(windows)]
use std::sync::{Condvar, OnceLock, mpsc};
#[cfg(any(unix, windows))]
use std::thread;
#[cfg(any(unix, windows))]
use std::time::Duration;

use pyo3::exceptions::PyOSError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

#[cfg(unix)]
use std::os::fd::{AsFd, AsRawFd, IntoRawFd};
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

const STDIN_CAPTURE_ERROR: &str = "stdin is unavailable while test output is captured";

fn write_all(os: &Bound<'_, PyModule>, py: Python<'_>, fd: i32, bytes: &[u8]) -> PyResult<()> {
    let write = os.getattr("write")?;
    let mut offset = 0;
    while offset < bytes.len() {
        let written = write
            .call1((fd, PyBytes::new(py, &bytes[offset..])))?
            .extract::<usize>()?;
        if written == 0 {
            return Err(PyOSError::new_err("os.write returned zero bytes"));
        }
        offset = offset.saturating_add(written.min(bytes.len() - offset));
    }
    Ok(())
}

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

/// Python text stream that writes directly to one runner-owned descriptor.
#[pyclass]
struct CapturedTextStream {
    os: Py<PyModule>,
    fd: i32,
}

#[pymethods]
#[expect(clippy::unused_self)]
impl CapturedTextStream {
    fn write(&self, py: Python<'_>, text: &str) -> PyResult<usize> {
        write_all(self.os.bind(py), py, self.fd, text.as_bytes())?;
        Ok(text.chars().count())
    }

    fn flush(&self) {}

    fn fileno(&self) -> i32 {
        self.fd
    }

    fn isatty(&self) -> bool {
        false
    }

    fn readable(&self) -> bool {
        false
    }

    fn writable(&self) -> bool {
        true
    }

    #[getter]
    fn encoding(&self) -> &'static str {
        "utf-8"
    }

    #[getter]
    fn errors(&self) -> &'static str {
        "replace"
    }

    #[getter]
    fn buffer(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Py::new(
            py,
            CapturedBinaryStream {
                os: self.os.clone_ref(py),
                fd: self.fd,
            },
        )
        .map(Py::into_any)
    }
}

/// Python binary stream paired with a runner-owned text stream.
#[pyclass]
struct CapturedBinaryStream {
    os: Py<PyModule>,
    fd: i32,
}

#[cfg(any(unix, windows))]
struct CapturedRing {
    limit: usize,
    head: Vec<u8>,
    tail: VecDeque<u8>,
    total: u64,
}

#[cfg(unix)]
struct UnixReaderControl {
    active: AtomicBool,
    done: Mutex<bool>,
    wake: Mutex<Option<thread::Thread>>,
    finished: Condvar,
}

#[cfg(unix)]
struct UnixReaderTask {
    reader: std::io::PipeReader,
    ring: Arc<Mutex<CapturedRing>>,
    control: Arc<UnixReaderControl>,
    error: Arc<Mutex<Option<String>>>,
}

#[cfg(unix)]
static UNIX_READER_POOL: OnceLock<mpsc::Sender<UnixReaderTask>> = OnceLock::new();

#[cfg(unix)]
fn unix_reader_sender() -> &'static mpsc::Sender<UnixReaderTask> {
    UNIX_READER_POOL.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<UnixReaderTask>();
        let receiver = Arc::new(Mutex::new(receiver));
        for _ in 0..2 {
            let receiver = Arc::clone(&receiver);
            thread::spawn(move || {
                loop {
                    let task = {
                        let receiver = match receiver.lock() {
                            Ok(receiver) => receiver,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        receiver.recv()
                    };
                    let Ok(task) = task else {
                        return;
                    };
                    let UnixReaderTask {
                        reader,
                        ring,
                        control,
                        error,
                    } = task;
                    if let Ok(mut wake) = control.wake.lock() {
                        *wake = Some(thread::current());
                    }
                    drain_native_pipe(reader, ring, Arc::clone(&control), error);
                    if let Ok(mut wake) = control.wake.lock() {
                        *wake = None;
                    }
                    let mut done = match control.done.lock() {
                        Ok(done) => done,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    *done = true;
                    control.finished.notify_all();
                }
            });
        }
        sender
    })
}

#[cfg(unix)]
/// Owns a bounded collector and a task assigned to the persistent reader pool.
struct NativePipe {
    ring: Arc<Mutex<CapturedRing>>,
    control: Arc<UnixReaderControl>,
    error: Arc<Mutex<Option<String>>>,
}

#[cfg(unix)]
impl NativePipe {
    fn new(reader: std::io::PipeReader, limit: usize) -> Self {
        let ring = Arc::new(Mutex::new(CapturedRing {
            limit: limit.max(1),
            head: Vec::new(),
            tail: VecDeque::new(),
            total: 0,
        }));
        let control = Arc::new(UnixReaderControl {
            active: AtomicBool::new(true),
            done: Mutex::new(false),
            wake: Mutex::new(None),
            finished: Condvar::new(),
        });
        let error = Arc::new(Mutex::new(None));
        let task = UnixReaderTask {
            reader,
            ring: Arc::clone(&ring),
            control: Arc::clone(&control),
            error: Arc::clone(&error),
        };
        if unix_reader_sender().send(task).is_err() {
            set_reader_error(&error, "output capture reader pool stopped".to_owned());
            if let Ok(mut done) = control.done.lock() {
                *done = true;
                control.finished.notify_all();
            }
        }
        Self {
            ring,
            control,
            error,
        }
    }

    fn stop_and_snapshot(&self, _py: Python<'_>) -> Result<(Vec<u8>, u64), String> {
        self.stop_reader();
        if let Some(error) = self
            .error
            .lock()
            .map_err(|_| "output capture error state is poisoned".to_owned())?
            .as_ref()
        {
            return Err(error.clone());
        }
        Ok(snapshot_ring(&self.ring))
    }

    fn stop_reader(&self) {
        self.control.active.store(false, Ordering::Relaxed);
        if let Ok(wake) = self.control.wake.lock()
            && let Some(wake) = wake.as_ref()
        {
            wake.unpark();
        }
        let mut done = match self.control.done.lock() {
            Ok(done) => done,
            Err(poisoned) => poisoned.into_inner(),
        };
        while !*done {
            done = match self.control.finished.wait(done) {
                Ok(done) => done,
                Err(poisoned) => poisoned.into_inner(),
            };
        }
    }
}

#[cfg(unix)]
impl Drop for NativePipe {
    fn drop(&mut self) {
        self.stop_reader();
    }
}

#[cfg(windows)]
struct WindowsReaderState {
    active: bool,
    remaining: Option<usize>,
}

#[cfg(windows)]
struct WindowsReaderControl {
    state: Mutex<WindowsReaderState>,
    done: Mutex<bool>,
    wake: Mutex<Option<thread::Thread>>,
    finished: Condvar,
}

#[cfg(windows)]
struct WindowsReaderTask {
    reader: std::io::PipeReader,
    ring: Arc<Mutex<CapturedRing>>,
    control: Arc<WindowsReaderControl>,
    error: Arc<Mutex<Option<String>>>,
}

#[cfg(windows)]
static WINDOWS_READER_POOL: OnceLock<mpsc::Sender<WindowsReaderTask>> = OnceLock::new();

#[cfg(windows)]
fn windows_reader_sender() -> &'static mpsc::Sender<WindowsReaderTask> {
    WINDOWS_READER_POOL.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<WindowsReaderTask>();
        let receiver = Arc::new(Mutex::new(receiver));
        for _ in 0..2 {
            let receiver = Arc::clone(&receiver);
            thread::spawn(move || {
                loop {
                    let task = {
                        let receiver = match receiver.lock() {
                            Ok(receiver) => receiver,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        receiver.recv()
                    };
                    let Ok(task) = task else {
                        return;
                    };
                    let WindowsReaderTask {
                        reader,
                        ring,
                        control,
                        error,
                    } = task;
                    if let Ok(mut wake) = control.wake.lock() {
                        *wake = Some(thread::current());
                    }
                    drain_windows_pipe(reader, ring, Arc::clone(&control), error);
                    if let Ok(mut wake) = control.wake.lock() {
                        *wake = None;
                    }
                    let mut done = match control.done.lock() {
                        Ok(done) => done,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    *done = true;
                    control.finished.notify_all();
                }
            });
        }
        sender
    })
}

#[cfg(windows)]
/// Owns a bounded collector and a task assigned to the persistent native reader pool.
struct NativePipe {
    ring: Arc<Mutex<CapturedRing>>,
    control: Arc<WindowsReaderControl>,
    error: Arc<Mutex<Option<String>>>,
    handle: usize,
}

#[cfg(windows)]
impl NativePipe {
    fn new(reader: std::io::PipeReader, handle: usize, limit: usize) -> Self {
        let ring = Arc::new(Mutex::new(CapturedRing {
            limit: limit.max(1),
            head: Vec::new(),
            tail: VecDeque::new(),
            total: 0,
        }));
        let control = Arc::new(WindowsReaderControl {
            state: Mutex::new(WindowsReaderState {
                active: true,
                remaining: None,
            }),
            done: Mutex::new(false),
            wake: Mutex::new(None),
            finished: Condvar::new(),
        });
        let error = Arc::new(Mutex::new(None));
        let task = WindowsReaderTask {
            reader,
            ring: Arc::clone(&ring),
            control: Arc::clone(&control),
            error: Arc::clone(&error),
        };
        if windows_reader_sender().send(task).is_err() {
            set_reader_error(&error, "output capture reader pool stopped".to_owned());
            if let Ok(mut done) = control.done.lock() {
                *done = true;
                control.finished.notify_all();
            }
        }
        Self {
            ring,
            control,
            error,
            handle,
        }
    }

    fn stop_and_snapshot(&self, py: Python<'_>) -> Result<(Vec<u8>, u64), String> {
        let ring = self
            .ring
            .lock()
            .map_err(|_| "output capture ring state is poisoned".to_owned())?;
        let available = py
            .import("_winapi")
            .and_then(|winapi| winapi.getattr("PeekNamedPipe"))
            .and_then(|peek| peek.call1((self.handle, 0)))
            .and_then(|value| value.extract::<(usize, usize)>())
            .map(|(available, _)| available)
            .map_err(|error| error.to_string());
        let available = match available {
            Ok(available) => available,
            Err(error) => {
                self.request_stop(0);
                drop(ring);
                self.wait_for_reader();
                return Err(error);
            }
        };
        self.request_stop(available);
        drop(ring);
        self.wait_for_reader();
        if let Some(error) = self
            .error
            .lock()
            .map_err(|_| "output capture error state is poisoned".to_owned())?
            .as_ref()
        {
            return Err(error.clone());
        }
        Ok(snapshot_ring(&self.ring))
    }

    fn request_stop(&self, remaining: usize) {
        if let Ok(mut state) = self.control.state.lock() {
            state.active = false;
            state.remaining = Some(remaining);
        }
        if let Ok(wake) = self.control.wake.lock()
            && let Some(wake) = wake.as_ref()
        {
            wake.unpark();
        }
    }

    fn wait_for_reader(&self) {
        let mut done = match self.control.done.lock() {
            Ok(done) => done,
            Err(poisoned) => poisoned.into_inner(),
        };
        while !*done {
            done = match self.control.finished.wait(done) {
                Ok(done) => done,
                Err(poisoned) => poisoned.into_inner(),
            };
        }
    }
}

#[cfg(windows)]
impl Drop for NativePipe {
    fn drop(&mut self) {
        self.request_stop(0);
        self.wait_for_reader();
    }
}

#[cfg(windows)]
fn drain_windows_pipe(
    mut reader: std::io::PipeReader,
    ring: Arc<Mutex<CapturedRing>>,
    control: Arc<WindowsReaderControl>,
    error: Arc<Mutex<Option<String>>>,
) {
    let mut bytes = vec![0_u8; 64 * 1024];
    loop {
        let (active, remaining, read_result) = {
            let mut ring = match ring.lock() {
                Ok(ring) => ring,
                Err(poisoned) => poisoned.into_inner(),
            };
            let mut state = match control.state.lock() {
                Ok(state) => state,
                Err(poisoned) => poisoned.into_inner(),
            };
            let active = state.active;
            let remaining = state.remaining;
            if !active && remaining == Some(0) {
                return;
            }
            let read_length = remaining.map_or(bytes.len(), |remaining| remaining.min(bytes.len()));
            let read_result = reader.read(&mut bytes[..read_length]);
            if let Ok(length) = read_result
                && length > 0
            {
                append_ring_locked(&mut ring, &bytes[..length]);
                if let Some(remaining) = remaining {
                    state.remaining = Some(remaining.saturating_sub(length));
                }
            }
            (active, remaining, read_result)
        };
        match read_result {
            Ok(0) => {
                if let Some(remaining) = remaining
                    && remaining > 0
                {
                    set_reader_error(
                        &error,
                        format!("output pipe closed with {remaining} bytes still queued"),
                    );
                }
                if !active || remaining.is_some() {
                    return;
                }
                thread::park_timeout(Duration::from_millis(1));
            }
            Ok(_) => {}
            Err(error_value) if is_windows_pipe_empty(&error_value) => {
                if !active && remaining.is_some() {
                    if remaining.is_some_and(|remaining| remaining > 0) {
                        set_reader_error(
                            &error,
                            format!("output pipe stopped with {remaining:?} bytes still queued"),
                        );
                    }
                    return;
                }
                thread::park_timeout(Duration::from_millis(1));
            }
            Err(error_value) => {
                set_reader_error(&error, error_value.to_string());
                return;
            }
        }
    }
}

#[cfg(windows)]
fn is_windows_pipe_empty(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::WouldBlock || error.raw_os_error() == Some(232)
}

#[cfg(unix)]
#[expect(clippy::needless_pass_by_value)]
fn drain_native_pipe(
    mut reader: std::io::PipeReader,
    ring: Arc<Mutex<CapturedRing>>,
    control: Arc<UnixReaderControl>,
    error: Arc<Mutex<Option<String>>>,
) {
    let mut bytes = vec![0_u8; 64 * 1024];
    loop {
        match reader.read(&mut bytes) {
            Ok(0) => {
                if !control.active.load(Ordering::Relaxed) {
                    drain_unix_available(&mut reader, &ring, &error, &mut bytes);
                    return;
                }
                thread::park_timeout(Duration::from_millis(1));
            }
            Ok(length) => append_ring(&ring, &bytes[..length]),
            Err(read_error) if read_error.kind() == io::ErrorKind::WouldBlock => {
                if !control.active.load(Ordering::Relaxed) {
                    drain_unix_available(&mut reader, &ring, &error, &mut bytes);
                    return;
                }
                thread::park_timeout(Duration::from_millis(1));
            }
            Err(error_value) => {
                set_reader_error(&error, error_value.to_string());
                return;
            }
        }
        if !control.active.load(Ordering::Relaxed) {
            drain_unix_available(&mut reader, &ring, &error, &mut bytes);
            return;
        }
    }
}

#[cfg(unix)]
fn drain_unix_available(
    reader: &mut std::io::PipeReader,
    ring: &Mutex<CapturedRing>,
    error: &Mutex<Option<String>>,
    bytes: &mut [u8],
) {
    let mut remaining = match rustix::io::ioctl_fionread(reader.as_fd()) {
        Ok(available) => usize::try_from(available).unwrap_or(usize::MAX),
        Err(error_value) => {
            set_reader_error(error, error_value.to_string());
            return;
        }
    };
    while remaining > 0 {
        let read_length = remaining.min(bytes.len());
        match reader.read(&mut bytes[..read_length]) {
            Ok(length) if length > 0 => {
                append_ring(ring, &bytes[..length]);
                remaining = remaining.saturating_sub(length);
            }
            Ok(_) => return,
            Err(error_value) if error_value.kind() == io::ErrorKind::WouldBlock => return,
            Err(error_value) => {
                set_reader_error(error, error_value.to_string());
                return;
            }
        }
    }
}

#[cfg(any(unix, windows))]
fn set_reader_error(error: &Mutex<Option<String>>, message: String) {
    if let Ok(mut error) = error.lock() {
        *error = Some(message);
    }
}

#[cfg(unix)]
fn append_ring(ring: &Mutex<CapturedRing>, bytes: &[u8]) {
    let mut ring = match ring.lock() {
        Ok(ring) => ring,
        Err(poisoned) => poisoned.into_inner(),
    };
    append_ring_locked(&mut ring, bytes);
}

#[cfg(any(unix, windows))]
fn append_ring_locked(ring: &mut CapturedRing, bytes: &[u8]) {
    ring.total = ring.total.saturating_add(bytes.len() as u64);
    let head_limit = ring.limit / 2;
    let head_remaining = head_limit.saturating_sub(ring.head.len());
    let head_bytes = head_remaining.min(bytes.len());
    ring.head.extend_from_slice(&bytes[..head_bytes]);
    let tail_limit = ring.limit - head_limit;
    let tail_start = head_bytes.max(bytes.len().saturating_sub(tail_limit));
    ring.tail.extend(bytes[tail_start..].iter().copied());
    while ring.tail.len() > tail_limit {
        ring.tail.pop_front();
    }
}

#[cfg(any(unix, windows))]
fn snapshot_ring(ring: &Mutex<CapturedRing>) -> (Vec<u8>, u64) {
    let ring = match ring.lock() {
        Ok(ring) => ring,
        Err(poisoned) => poisoned.into_inner(),
    };
    let mut retained = ring.head.clone();
    retained.extend(ring.tail.iter().copied());
    (retained, ring.total)
}

#[pymethods]
#[expect(clippy::unused_self)]
impl CapturedBinaryStream {
    fn write(&self, py: Python<'_>, bytes: &Bound<'_, PyBytes>) -> PyResult<usize> {
        write_all(self.os.bind(py), py, self.fd, bytes.as_bytes())?;
        Ok(bytes.as_bytes().len())
    }

    fn flush(&self) {}

    fn fileno(&self) -> i32 {
        self.fd
    }

    fn isatty(&self) -> bool {
        false
    }

    fn readable(&self) -> bool {
        false
    }

    fn writable(&self) -> bool {
        true
    }
}

/// Process-global Python stream redirection for one test attempt.
///
/// `start` replaces Python streams and descriptors 1 and 2 with per-attempt
/// bounded pipe collectors. Callers must consume the value with
/// [`Self::finish`] to restore every stream and return bounded text.
pub struct PythonOutputCapture {
    os: Py<PyModule>,
    sys: Py<PyModule>,
    old_stdout: Py<PyAny>,
    old_stderr: Py<PyAny>,
    #[cfg(any(unix, windows))]
    stdout_pipe: NativePipe,
    #[cfg(any(unix, windows))]
    stderr_pipe: NativePipe,
    old_stdout_fd: Option<i32>,
    old_stderr_fd: Option<i32>,
    output_limit: usize,
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
    pub fn start(py: Python<'_>, output_limit: usize) -> PyResult<Self> {
        let os = py.import("os")?.unbind();
        let sys = py.import("sys")?.unbind();
        let sys_bound = sys.bind(py);
        let os_bound = os.bind(py);

        let old_stdout = sys_bound.getattr("stdout")?.unbind();
        let old_stderr = sys_bound.getattr("stderr")?.unbind();

        let old_stdout_fd = duplicate_fd(os_bound, 1)?;
        let old_stderr_fd = match duplicate_fd(os_bound, 2) {
            Ok(fd) => fd,
            Err(error) => {
                close_optional_fd(os_bound, old_stdout_fd);
                return Err(error);
            }
        };
        #[cfg(any(unix, windows))]
        let (stdout_pipe, stdout_fd) = match capture_pipe(py, &os, output_limit) {
            Ok(pipe) => pipe,
            Err(error) => {
                close_optional_fd(os_bound, old_stdout_fd);
                close_optional_fd(os_bound, old_stderr_fd);
                return Err(error);
            }
        };
        #[cfg(unix)]
        let (stderr_pipe, stderr_fd) = match capture_pipe(py, &os, output_limit) {
            Ok(pipe) => pipe,
            Err(error) => {
                close_pipe_write_fd(stdout_fd, os_bound, 1);
                close_optional_fd(os_bound, old_stdout_fd);
                close_optional_fd(os_bound, old_stderr_fd);
                return Err(error);
            }
        };
        #[cfg(windows)]
        let (stderr_pipe, stderr_fd) = match capture_pipe(py, &os, output_limit) {
            Ok(pipe) => pipe,
            Err(error) => {
                close_pipe_write_fd(stdout_fd, os_bound, 1);
                close_optional_fd(os_bound, old_stdout_fd);
                close_optional_fd(os_bound, old_stderr_fd);
                return Err(error);
            }
        };
        let stdout = match Py::new(
            py,
            CapturedTextStream {
                os: os.clone_ref(py),
                fd: 1,
            },
        ) {
            Ok(stream) => stream.into_any(),
            Err(error) => {
                close_pipe_write_fd(stdout_fd, os_bound, 1);
                close_pipe_write_fd(stderr_fd, os_bound, 2);
                close_optional_fd(os_bound, old_stdout_fd);
                close_optional_fd(os_bound, old_stderr_fd);
                return Err(error);
            }
        };
        let stderr = match Py::new(
            py,
            CapturedTextStream {
                os: os.clone_ref(py),
                fd: 2,
            },
        ) {
            Ok(stream) => stream.into_any(),
            Err(error) => {
                close_pipe_write_fd(stdout_fd, os_bound, 1);
                close_pipe_write_fd(stderr_fd, os_bound, 2);
                close_optional_fd(os_bound, old_stdout_fd);
                close_optional_fd(os_bound, old_stderr_fd);
                return Err(error);
            }
        };
        if let Err(error) = os_bound.getattr("dup2")?.call1((stdout_fd, 1)) {
            close_pipe_write_fd(stdout_fd, os_bound, 1);
            close_pipe_write_fd(stderr_fd, os_bound, 2);
            close_optional_fd(os_bound, old_stdout_fd);
            close_optional_fd(os_bound, old_stderr_fd);
            return Err(error);
        }
        if let Err(error) = os_bound.getattr("dup2")?.call1((stderr_fd, 2)) {
            close_pipe_write(stdout_fd, os_bound, 1);
            close_pipe_write(stderr_fd, os_bound, 2);
            let _ = restore_fd(os_bound, 1, old_stdout_fd);
            close_optional_fd(os_bound, old_stderr_fd);
            return Err(error);
        }
        #[cfg(unix)]
        {
            close_pipe_write(stdout_fd, os_bound, 1);
            close_pipe_write(stderr_fd, os_bound, 2);
        }
        #[cfg(windows)]
        {
            close_pipe_write(stdout_fd, os_bound, 1);
            close_pipe_write(stderr_fd, os_bound, 2);
        }

        if let Err(error) = sys_bound.setattr("stdout", stdout.bind(py)) {
            let _ = restore_fd(os_bound, 1, old_stdout_fd);
            let _ = restore_fd(os_bound, 2, old_stderr_fd);
            return Err(error);
        }
        if let Err(err) = sys_bound.setattr("stderr", stderr.bind(py)) {
            if let Err(restore_err) = sys_bound.setattr("stdout", old_stdout.bind(py)) {
                tracing::warn!(
                    "failed to restore Python stdout after capture setup error: {restore_err}"
                );
            }
            let _ = restore_fd(os_bound, 1, old_stdout_fd);
            let _ = restore_fd(os_bound, 2, old_stderr_fd);
            return Err(err);
        }
        Ok(Self {
            os,
            sys,
            old_stdout,
            old_stderr,
            stdout_pipe,
            stderr_pipe,
            old_stdout_fd,
            old_stderr_fd,
            output_limit: output_limit.max(1),
        })
    }

    /// Flushes captured streams, restores their original objects, and returns captured text.
    pub fn finish(self, py: Python<'_>) -> PyResult<CapturedPythonOutput> {
        let sys = self.sys.bind(py);
        let os = self.os.bind(py);
        flush_current_streams(sys);

        let restore_result = restore_stdio(sys, &self.old_stdout, &self.old_stderr, py);
        // Restore the process descriptors before stopping the readers. This
        // closes the test writer in this process, so output emitted after the
        // lifecycle boundary cannot race the final bounded snapshot.
        let stdout_restore = restore_fd(os, 1, self.old_stdout_fd);
        let stderr_restore = restore_fd(os, 2, self.old_stderr_fd);
        #[cfg(unix)]
        let native_stdout = snapshot_captured_pipe(py, &self.stdout_pipe, self.output_limit);
        #[cfg(unix)]
        let native_stderr = snapshot_captured_pipe(py, &self.stderr_pipe, self.output_limit);
        #[cfg(windows)]
        let native_stdout = snapshot_captured_pipe(py, &self.stdout_pipe, self.output_limit);
        #[cfg(windows)]
        let native_stderr = snapshot_captured_pipe(py, &self.stderr_pipe, self.output_limit);
        restore_result?;
        stdout_restore?;
        stderr_restore?;

        let (stdout, stdout_raw, stdout_total) = native_stdout?;
        let (stderr, stderr_raw, stderr_total) = native_stderr?;
        Ok(CapturedPythonOutput {
            stdout,
            stderr,
            stdout_raw,
            stderr_raw,
            stdout_total,
            stderr_total,
        })
    }
}

/// Text emitted through Python's stdout and stderr during one capture window.
pub struct CapturedPythonOutput {
    /// Captured stdout, preserving write order within that stream.
    pub stdout: String,

    /// Captured stderr, preserving write order within that stream.
    pub stderr: String,

    /// Bounded raw stdout retained for aggregate retry output.
    pub stdout_raw: Vec<u8>,

    /// Bounded raw stderr retained for aggregate retry output.
    pub stderr_raw: Vec<u8>,

    /// Raw stdout bytes observed before bounded retention.
    pub stdout_total: u64,

    /// Raw stderr bytes observed before bounded retention.
    pub stderr_total: u64,
}

#[cfg(unix)]
fn capture_pipe(py: Python<'_>, os: &Py<PyModule>, limit: usize) -> PyResult<(NativePipe, i32)> {
    let (reader, writer) = std::io::pipe()
        .map_err(|error| PyOSError::new_err(format!("failed to create output pipe: {error}")))?;
    let reader_fd = reader.as_raw_fd();
    os.bind(py)
        .getattr("set_blocking")?
        .call1((reader_fd, false))?;
    let writer_fd = writer.into_raw_fd();
    Ok((NativePipe::new(reader, limit), writer_fd))
}

#[cfg(windows)]
fn capture_pipe(py: Python<'_>, os: &Py<PyModule>, limit: usize) -> PyResult<(NativePipe, i32)> {
    let (reader, writer) = std::io::pipe()
        .map_err(|error| PyOSError::new_err(format!("failed to create output pipe: {error}")))?;
    let reader_handle = reader.as_raw_handle() as usize;
    let winapi = py.import("_winapi")?;
    let mode = winapi.getattr("PIPE_NOWAIT")?;
    winapi.getattr("SetNamedPipeHandleState")?.call1((
        reader_handle,
        mode,
        py.None(),
        py.None(),
    ))?;
    let os = os.bind(py);
    let flags = os.getattr("O_WRONLY")?;
    let msvcrt = py.import("msvcrt")?;
    let writer_handle = writer.as_raw_handle();
    let writer_fd = match msvcrt
        .getattr("open_osfhandle")?
        .call1((writer_handle as isize, flags))
        .and_then(|value| value.extract::<i32>())
    {
        Ok(fd) => fd,
        Err(error) => return Err(error),
    };
    std::mem::forget(writer);
    Ok((NativePipe::new(reader, reader_handle, limit), writer_fd))
}

#[cfg(any(unix, windows))]
fn close_pipe_write(write_fd: i32, os: &Bound<'_, PyModule>, target: i32) {
    if write_fd != target {
        let _ = os
            .getattr("close")
            .and_then(|close| close.call1((write_fd,)));
    }
}

#[cfg(any(unix, windows))]
fn close_pipe_write_fd(write_fd: i32, os: &Bound<'_, PyModule>, target: i32) {
    close_pipe_write(write_fd, os, target);
}

fn duplicate_fd(os: &Bound<'_, PyModule>, fd: i32) -> PyResult<Option<i32>> {
    match os
        .getattr("dup")?
        .call1((fd,))
        .and_then(|value| value.extract())
    {
        Ok(fd) => Ok(Some(fd)),
        Err(error) if is_bad_fd(os, &error)? => Ok(None),
        Err(error) => Err(error),
    }
}

fn close_optional_fd(os: &Bound<'_, PyModule>, fd: Option<i32>) {
    if let Some(fd) = fd {
        let _ = os.getattr("close").and_then(|close| close.call1((fd,)));
    }
}

fn restore_fd(os: &Bound<'_, PyModule>, target: i32, old_fd: Option<i32>) -> PyResult<()> {
    match old_fd {
        Some(old_fd) => {
            let result = os.getattr("dup2")?.call1((old_fd, target));
            let close_result = os.getattr("close")?.call1((old_fd,));
            result.and(close_result).map(|_| ())
        }
        None => match os.getattr("close")?.call1((target,)) {
            Ok(_) => Ok(()),
            Err(error) if is_bad_fd(os, &error)? => Ok(()),
            Err(error) => Err(error),
        },
    }
}

#[cfg(unix)]
fn snapshot_captured_pipe(
    py: Python<'_>,
    pipe: &NativePipe,
    output_limit: usize,
) -> PyResult<(String, Vec<u8>, u64)> {
    let (bytes, total) = pipe.stop_and_snapshot(py).map_err(PyOSError::new_err)?;
    let raw = bytes.clone();
    Ok((
        format_captured_bytes(&bytes, total, output_limit),
        raw,
        total,
    ))
}

#[cfg(windows)]
fn snapshot_captured_pipe(
    py: Python<'_>,
    pipe: &NativePipe,
    output_limit: usize,
) -> PyResult<(String, Vec<u8>, u64)> {
    let (bytes, total) = pipe.stop_and_snapshot(py).map_err(PyOSError::new_err)?;
    let raw = bytes.clone();
    Ok((
        format_captured_bytes(&bytes, total, output_limit),
        raw,
        total,
    ))
}

/// Formats bounded raw output and reports the raw byte total.
pub fn format_captured_bytes(bytes: &[u8], total: u64, output_limit: usize) -> String {
    if total <= output_limit as u64 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let omitted = total.saturating_sub(bytes.len() as u64);
    let head_len = output_limit / 2;
    let head = &bytes[..head_len.min(bytes.len())];
    let tail_start = bytes.len().saturating_sub(output_limit - head_len);
    let tail = &bytes[tail_start..];
    format!(
        "[Karva output truncated: {total} bytes captured, {omitted} bytes omitted]\n{}\n... output truncated ...\n{}",
        String::from_utf8_lossy(head),
        String::from_utf8_lossy(tail),
    )
}

/// Retains bounded head and tail bytes without adding a diagnostic marker.
pub fn retain_bounded_bytes(bytes: Vec<u8>, output_limit: usize) -> Vec<u8> {
    if bytes.len() <= output_limit {
        return bytes;
    }
    let head_limit = output_limit / 2;
    let tail_start = bytes.len().saturating_sub(output_limit - head_limit);
    let mut retained = Vec::new();
    retained.extend_from_slice(&bytes[..head_limit]);
    retained.extend_from_slice(&bytes[tail_start..]);
    retained
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
    let stdout_result = sys.setattr("stdout", stdout.bind(py));
    let stderr_result = sys.setattr("stderr", stderr.bind(py));
    stdout_result.and(stderr_result)
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
