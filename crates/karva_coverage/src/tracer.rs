//! Worker-side line tracer.
//!
//! Installs a Python tracer that records every executed line under the
//! configured source roots, then on stop computes executable lines for each
//! touched file and writes a per-worker JSON file at
//! [`CoverageConfig::data_file`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use camino::{Utf8Path, Utf8PathBuf};
use fs_err as fs;
use pyo3::prelude::*;

use crate::data::{FileEntry, WorkerFile};
use crate::executable::executable_lines;

/// Configuration for a single worker's coverage measurement.
#[derive(Debug, Clone)]
pub struct CoverageConfig {
    /// Source paths to measure. An empty entry means "measure the current
    /// working directory" (matches pytest-cov's bare `--cov`).
    pub sources: Vec<String>,

    /// Per-worker data file path. The runner combines these after the run.
    pub data_file: Utf8PathBuf,
}

/// Path components inside a source root that suppress tracking. These match
/// the conventional locations of installed third-party code.
const PATH_EXCLUDES: &[&str] = &["site-packages", "dist-packages", ".venv", ".tox"];

/// A live coverage measurement. Drop without calling [`Self::stop_and_save`]
/// to abandon a partial run; the data file is only persisted via
/// `stop_and_save`.
pub struct CoverageSession {
    tracer: Py<CoverageTracer>,
    data_file: Utf8PathBuf,
}

impl CoverageSession {
    pub fn start(py: Python<'_>, cwd: &Utf8Path, config: &CoverageConfig) -> PyResult<Self> {
        let roots: Vec<PathBuf> = config
            .sources
            .iter()
            .map(|s| {
                let raw = if s.is_empty() {
                    cwd.as_str()
                } else {
                    s.as_str()
                };
                fs::canonicalize(raw).unwrap_or_else(|_| PathBuf::from(raw))
            })
            .collect();

        let tracer = Py::new(
            py,
            CoverageTracer {
                roots,
                state: Mutex::new(TracerState::default()),
                monitoring_tool_id: OnceLock::new(),
                monitoring_disable: OnceLock::new(),
            },
        )?;

        if py_version_at_least(py, 3, 12)? {
            install_monitoring(py, &tracer)?;
        } else {
            install_settrace(py, &tracer)?;
        }

        Ok(Self {
            tracer,
            data_file: config.data_file.clone(),
        })
    }

    pub fn stop_and_save(self, py: Python<'_>) -> PyResult<()> {
        let Self { tracer, data_file } = self;
        let bound = tracer.bind(py);
        let tool_id = bound.borrow().monitoring_tool_id.get().copied();

        if let Some(tool_id) = tool_id {
            let mon = py.import("sys")?.getattr("monitoring")?;
            let line_event = mon.getattr("events")?.getattr("LINE")?;
            mon.call_method1("set_events", (tool_id, 0u32))?;
            mon.call_method1("register_callback", (tool_id, line_event, py.None()))?;
            mon.call_method1("free_tool_id", (tool_id,))?;
        } else {
            py.import("sys")?.call_method1("settrace", (py.None(),))?;
        }

        let borrowed = bound.borrow();
        let executed = match borrowed.state.lock() {
            Ok(mut state) => std::mem::take(&mut state.executed),
            Err(poisoned) => std::mem::take(&mut poisoned.into_inner().executed),
        };
        let roots = borrowed.roots.clone();
        drop(borrowed);
        save_data(&data_file, executed, &roots).map_err(|err| {
            pyo3::exceptions::PyOSError::new_err(format!(
                "failed to write coverage data to {data_file}: {err}"
            ))
        })?;
        Ok(())
    }
}

#[derive(Default)]
struct TracerState {
    /// Files with the set of executed line numbers.
    executed: HashMap<PathBuf, HashSet<u32>>,
    /// Memoized result of [`compute_tracked_path`] per filename string.
    track_cache: HashMap<String, Option<PathBuf>>,
    /// Memoized result of [`compute_tracked_path`] per live Python code object.
    code_cache: HashMap<usize, TrackedCode>,
}

struct TrackedCode {
    code: Py<PyAny>,
    path: Option<PathBuf>,
}

/// Thread-safe because the trace callbacks fire on whichever Python thread
/// happens to be executing tracked code: `sys.monitoring` LINE events are
/// global to the registered tool id, and `sys.settrace` propagates to threads
/// that opt in via `threading.settrace`. Marking the pyclass `unsendable`
/// panics in `borrow()` as soon as a Python thread other than the installer
/// invokes a callback (issue #760).
#[pyclass(module = "karva_coverage")]
struct CoverageTracer {
    roots: Vec<PathBuf>,
    state: Mutex<TracerState>,
    monitoring_tool_id: OnceLock<u8>,
    /// Cached `sys.monitoring.DISABLE` sentinel. Populated when the
    /// `sys.monitoring` backend is installed; never accessed for the
    /// `sys.settrace` backend. Caching avoids importing `sys` inside the
    /// hot callback, which can re-enter the import system while `CPython`
    /// is mid-import and surface as `KeyError('__import__')`.
    monitoring_disable: OnceLock<Py<PyAny>>,
}

#[pymethods]
impl CoverageTracer {
    /// `sys.monitoring` LINE event callback. Records the line if it's in a
    /// tracked file, then always returns `sys.monitoring.DISABLE` so the
    /// interpreter never calls us back for the same `(code, line)` pair.
    fn line_cb(
        &self,
        py: Python<'_>,
        code: &Bound<'_, PyAny>,
        lineno: u32,
    ) -> PyResult<Option<Py<PyAny>>> {
        if let Some(path) = self.tracked_code_path(code)?
            && let Ok(mut state) = self.state.lock()
        {
            state.executed.entry(path).or_default().insert(lineno);
        }
        Ok(self.monitoring_disable.get().map(|d| d.clone_ref(py)))
    }

    /// `sys.settrace` global trace function. Returns the per-frame
    /// [`Self::local_trace`] when the frame's file is under a source root.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "PyO3 requires Bound<Self> by value as a self receiver"
    )]
    fn trace<'py>(
        slf: Bound<'py, Self>,
        frame: &Bound<'py, PyAny>,
        event: &str,
        _arg: &Bound<'py, PyAny>,
    ) -> PyResult<Option<Py<PyAny>>> {
        if event == "call" {
            let filename: String = frame.getattr("f_code")?.getattr("co_filename")?.extract()?;
            if slf.borrow().tracked_path(&filename).is_some() {
                return Ok(Some(slf.getattr("local_trace")?.unbind()));
            }
        }
        Ok(None)
    }

    /// `sys.settrace` per-frame trace function. Records `line` events and
    /// returns itself so Python keeps tracing the frame.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "PyO3 requires Bound<Self> by value as a self receiver"
    )]
    fn local_trace<'py>(
        slf: Bound<'py, Self>,
        frame: &Bound<'py, PyAny>,
        event: &str,
        _arg: &Bound<'py, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        if event == "line" {
            let filename: String = frame.getattr("f_code")?.getattr("co_filename")?.extract()?;
            let path = slf.borrow().tracked_path(&filename);
            if let Some(path) = path {
                let lineno: u32 = frame.getattr("f_lineno")?.extract()?;
                if let Ok(mut state) = slf.borrow().state.lock() {
                    state.executed.entry(path).or_default().insert(lineno);
                }
            }
        }
        Ok(slf.getattr("local_trace")?.unbind())
    }
}

impl CoverageTracer {
    /// Resolve a live Python code object without extracting `co_filename`
    /// after the first line callback for that object.
    fn tracked_code_path(&self, code: &Bound<'_, PyAny>) -> PyResult<Option<PathBuf>> {
        let code_id = code.as_ptr() as usize;
        if let Ok(state) = self.state.lock()
            && let Some(cached) = state.code_cache.get(&code_id)
        {
            debug_assert!(cached.code.is(code));
            return Ok(cached.path.clone());
        }

        let filename: String = code.getattr("co_filename")?.extract()?;
        let path = self.tracked_path(&filename);

        if let Ok(mut state) = self.state.lock() {
            state.code_cache.insert(
                code_id,
                TrackedCode {
                    code: code.clone().unbind(),
                    path: path.clone(),
                },
            );
        }

        Ok(path)
    }

    /// Resolve `filename` against the source roots. Returns the canonical
    /// path if the file should be tracked, or `None` otherwise. Memoized
    /// per filename string.
    fn tracked_path(&self, filename: &str) -> Option<PathBuf> {
        if let Ok(state) = self.state.lock()
            && let Some(cached) = state.track_cache.get(filename)
        {
            return cached.clone();
        }
        let resolved = compute_tracked_path(filename, &self.roots);
        if let Ok(mut state) = self.state.lock() {
            state
                .track_cache
                .insert(filename.to_string(), resolved.clone());
        }
        resolved
    }
}

fn compute_tracked_path(filename: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    if filename.is_empty() || filename.starts_with('<') {
        return None;
    }
    let canonical = fs::canonicalize(filename).ok()?;
    if canonical
        .components()
        .any(|c| PATH_EXCLUDES.contains(&c.as_os_str().to_str().unwrap_or("")))
    {
        return None;
    }
    for root in roots {
        if canonical == *root || canonical.starts_with(root) {
            return Some(canonical);
        }
    }
    None
}

fn py_version_at_least(py: Python<'_>, major: u8, minor: u8) -> PyResult<bool> {
    let info = py.import("sys")?.getattr("version_info")?;
    let actual_major: u8 = info.get_item(0)?.extract()?;
    let actual_minor: u8 = info.get_item(1)?.extract()?;
    Ok((actual_major, actual_minor) >= (major, minor))
}

fn install_monitoring(py: Python<'_>, tracer: &Py<CoverageTracer>) -> PyResult<()> {
    let mon = py.import("sys")?.getattr("monitoring")?;
    let line_event = mon.getattr("events")?.getattr("LINE")?;
    let disable = mon.getattr("DISABLE")?.unbind();

    let tool_id = (0u8..6u8)
        .find(|id| mon.call_method1("use_tool_id", (*id, "karva")).is_ok())
        .ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err(
                "no free sys.monitoring tool id available for coverage",
            )
        })?;

    let install_result = (|| -> PyResult<()> {
        let callback = tracer.bind(py).getattr("line_cb")?;
        mon.call_method1("register_callback", (tool_id, &line_event, callback))?;
        mon.call_method1("set_events", (tool_id, &line_event))?;
        {
            let bound = tracer.bind(py).borrow();
            bound.monitoring_tool_id.set(tool_id).map_err(|_| {
                pyo3::exceptions::PyRuntimeError::new_err(
                    "coverage monitoring tool id was already initialized",
                )
            })?;
            bound.monitoring_disable.set(disable).map_err(|_| {
                pyo3::exceptions::PyRuntimeError::new_err(
                    "coverage monitoring disable sentinel was already initialized",
                )
            })?;
        }
        Ok(())
    })();

    if let Err(err) = install_result {
        release_monitoring_tool(py, &mon, &line_event, tool_id);
        return Err(err);
    }

    Ok(())
}

fn release_monitoring_tool(
    py: Python<'_>,
    mon: &Bound<'_, PyAny>,
    line_event: &Bound<'_, PyAny>,
    tool_id: u8,
) {
    if let Err(err) = mon.call_method1("set_events", (tool_id, 0u32)) {
        tracing::warn!("failed to disable sys.monitoring events during cleanup: {err}");
    }
    if let Err(err) = mon.call_method1("register_callback", (tool_id, line_event, py.None())) {
        tracing::warn!("failed to unregister sys.monitoring callback during cleanup: {err}");
    }
    if let Err(err) = mon.call_method1("free_tool_id", (tool_id,)) {
        tracing::warn!("failed to free sys.monitoring tool id during cleanup: {err}");
    }
}

fn install_settrace(py: Python<'_>, tracer: &Py<CoverageTracer>) -> PyResult<()> {
    let trace = tracer.bind(py).getattr("trace")?;
    py.import("sys")?.call_method1("settrace", (trace,))?;
    Ok(())
}

/// Walk source roots collecting `.py` files so that files which were never
/// imported during the run still appear in the report at 0% coverage.
/// Skips directories matching [`PATH_EXCLUDES`] and never follows symlinks
/// (avoids descending into a symlinked `.venv`).
fn walk_source_files(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for root in roots {
        let metadata = match fs::symlink_metadata(root) {
            Ok(metadata) => metadata,
            Err(err) => {
                tracing::warn!(
                    path = %root.display(),
                    "failed to inspect coverage source root: {err}"
                );
                continue;
            }
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_file() {
            if is_python_source(root) && seen.insert(root.clone()) {
                out.push(root.clone());
            }
        } else if metadata.is_dir() {
            walk_dir(root, &mut out, &mut seen);
        }
    }
    out
}

fn walk_dir(dir: &Path, out: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::warn!(
                path = %dir.display(),
                "failed to read coverage source directory: {err}"
            );
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                tracing::warn!(
                    path = %dir.display(),
                    "failed to read coverage source directory entry: {err}"
                );
                continue;
            }
        };
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) => {
                tracing::warn!(
                    path = %entry.path().display(),
                    "failed to inspect coverage source path: {err}"
                );
                continue;
            }
        };
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if PATH_EXCLUDES.contains(&name) {
                continue;
            }
            walk_dir(&path, out, seen);
        } else if file_type.is_file() && is_python_source(&path) && seen.insert(path.clone()) {
            out.push(path);
        }
    }
}

fn is_python_source(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("py")
}

fn save_data(
    data_file: &Utf8Path,
    mut executed: HashMap<PathBuf, HashSet<u32>>,
    roots: &[PathBuf],
) -> std::io::Result<()> {
    for path in walk_source_files(roots) {
        executed.entry(path).or_default();
    }

    let mut files = BTreeMap::new();
    for (path, hits) in executed {
        let executable = executable_lines(&path)?;
        if executable.is_empty() {
            continue;
        }
        let mut executed_lines: Vec<u32> = hits.intersection(&executable).copied().collect();
        executed_lines.sort_unstable();
        let mut executable_lines_vec: Vec<u32> = executable.into_iter().collect();
        executable_lines_vec.sort_unstable();
        files.insert(
            path.to_string_lossy().into_owned(),
            FileEntry {
                executable: executable_lines_vec,
                executed: executed_lines,
            },
        );
    }

    if let Some(parent) = data_file.parent()
        && !parent.as_str().is_empty()
    {
        fs::create_dir_all(parent.as_std_path())?;
    }
    let bytes = serde_json::to_vec(&WorkerFile { files })?;
    fs::write(data_file.as_std_path(), bytes)
}

#[cfg(test)]
mod tests {
    use pyo3::ffi::c_str;
    use pyo3::types::PyDict;

    use super::*;

    #[test]
    fn tracked_code_path_uses_code_cache_after_first_lookup() {
        let dir = tempfile::tempdir().expect("temp dir");
        let source = dir.path().join("module.py");
        fs::write(&source, "x = 1\n").expect("write source");
        let root = fs::canonicalize(dir.path()).expect("canonical root");
        let expected = Some(fs::canonicalize(&source).expect("canonical source"));

        Python::initialize();
        Python::attach(|py| -> PyResult<()> {
            let tracer = CoverageTracer {
                roots: vec![root],
                state: Mutex::new(TracerState::default()),
                monitoring_tool_id: OnceLock::new(),
                monitoring_disable: OnceLock::new(),
            };
            let locals = PyDict::new(py);
            locals.set_item("filename", source.to_string_lossy().as_ref())?;
            py.run(
                c_str!(
                    r#"
class Code:
    def __init__(self):
        self.calls = 0

    @property
    def co_filename(self):
        self.calls += 1
        if self.calls > 1:
            raise AssertionError("co_filename should be cached")
        return filename

code = Code()
"#
                ),
                Some(&locals),
                Some(&locals),
            )?;
            let code = locals.get_item("code")?.expect("code object");

            assert_eq!(tracer.tracked_code_path(&code)?, expected);
            assert_eq!(tracer.tracked_code_path(&code)?, expected);

            let calls: u32 = code.getattr("calls")?.extract()?;
            assert_eq!(calls, 1);

            let state = tracer.state.lock().expect("state lock");
            let cached = state
                .code_cache
                .get(&(code.as_ptr() as usize))
                .expect("cached code");
            assert!(cached.code.is(&code));

            Ok(())
        })
        .expect("python assertions");
    }

    #[test]
    fn save_data_reports_missing_executed_source() {
        let dir = tempfile::tempdir().expect("temp dir");
        let data_file = Utf8Path::from_path(dir.path())
            .expect("utf8 temp dir")
            .join("coverage.json");
        let missing = dir.path().join("missing.py");
        let executed = HashMap::from([(missing, HashSet::from([1]))]);

        let err = save_data(&data_file, executed, &[]).expect_err("missing source should fail");

        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(err.to_string().contains("missing.py"), "{err}");
        assert!(!data_file.exists());
    }
}
