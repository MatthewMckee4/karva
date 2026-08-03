//! Worker-side line tracer.
//!
//! Installs a Python tracer that records every executed line under the
//! configured source roots, then on stop computes executable lines for each
//! touched file and writes a per-worker JSON file at
//! [`CoverageConfig::data_file`].

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use camino::{Utf8Path, Utf8PathBuf};
use fs_err as fs;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::branches::{CoveragePartials, branch_analysis_with_exclusions};
use crate::context::{PENDING_SETUP_CONTEXT, SESSION_CONTEXT, compose_context};
use crate::data::{BranchArc, BranchContextEntry, BranchEntry, FileEntry, WorkerFile};
use crate::executable::{CoverageExclusions, executable_lines_with_exclusions};

/// Configuration for a single worker's coverage measurement.
#[derive(Debug, Clone)]
pub struct CoverageConfig {
    /// Source paths to measure. An empty entry means "measure the current
    /// working directory" (matches pytest-cov's bare `--cov`).
    pub sources: Vec<String>,

    /// Per-worker data file path. The runner combines these after the run.
    pub data_file: Utf8PathBuf,

    /// Whether to record the current test context for each executed line.
    pub contexts: bool,

    /// User-provided context attached to every observation in this run.
    pub static_context: Option<String>,

    /// Whether to record branch arcs in addition to executed lines.
    pub branches: bool,

    /// Regular expressions excluding matched source lines and clauses.
    pub exclude_lines: Vec<String>,

    /// Regular expressions suppressing missing arcs from matched branch lines.
    pub partial_branches: Vec<String>,
}

/// Test lifecycle phase recorded in a dynamic coverage context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoveragePhase {
    /// Function-scoped fixture setup.
    Setup,
    /// Test function execution.
    Run,
    /// Function-scoped fixture teardown.
    Teardown,
}

impl CoveragePhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Run => "run",
            Self::Teardown => "teardown",
        }
    }
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
    exclusions: CoverageExclusions,
    partials: CoveragePartials,
    include_unexecuted: bool,
}

/// Native coverage session owned by an opted-in Python child interpreter.
#[pyclass(name = "_ChildCoverageSession", module = "karva._karva")]
pub struct ChildCoverageSession {
    session: Option<CoverageSession>,
}

#[pymethods]
impl ChildCoverageSession {
    /// Stops collection and atomically writes this child's coverage shard.
    fn stop_and_save(&mut self, py: Python<'_>) -> PyResult<()> {
        if let Some(session) = self.session.take() {
            session.stop_and_save(py)?;
        }
        Ok(())
    }
}

/// Starts native coverage inside an opted-in Python child interpreter.
#[pyfunction(name = "_start_child_coverage")]
pub fn start_child_coverage(
    py: Python<'_>,
    roots: Vec<String>,
    data_file: String,
    branches: bool,
    exclude_lines: Vec<String>,
    partial_branches: Vec<String>,
    static_context: Option<String>,
) -> PyResult<ChildCoverageSession> {
    let config = CoverageConfig {
        sources: roots,
        data_file: Utf8PathBuf::from(data_file),
        contexts: false,
        static_context,
        branches,
        exclude_lines,
        partial_branches,
    };
    let session = CoverageSession::start_inner(py, Utf8Path::new(""), &config, false)?;
    Ok(ChildCoverageSession {
        session: Some(session),
    })
}

impl CoverageSession {
    /// Installs the best tracer supported by the embedded Python version.
    pub fn start(py: Python<'_>, cwd: &Utf8Path, config: &CoverageConfig) -> PyResult<Self> {
        Self::start_inner(py, cwd, config, true)
    }

    fn start_inner(
        py: Python<'_>,
        cwd: &Utf8Path,
        config: &CoverageConfig,
        include_unexecuted: bool,
    ) -> PyResult<Self> {
        let exclusions = CoverageExclusions::new(&config.exclude_lines)
            .map_err(|error| pyo3::exceptions::PyValueError::new_err(error.to_string()))?;
        let partials = CoveragePartials::new(&config.partial_branches)
            .map_err(|error| pyo3::exceptions::PyValueError::new_err(error.to_string()))?;
        let roots = resolve_source_roots(py, cwd, &config.sources)?;
        let record_contexts = config.contexts || config.static_context.is_some();
        let initial_context = if config.contexts {
            compose_context(config.static_context.as_deref(), &[SESSION_CONTEXT])
        } else {
            compose_context(config.static_context.as_deref(), &[])
        };

        let tracer = Py::new(
            py,
            CoverageTracer {
                roots,
                contexts: record_contexts,
                test_contexts: config.contexts,
                static_context: config.static_context.clone(),
                branches: config.branches,
                state: Mutex::new(TracerState {
                    current_context: initial_context,
                    ..TracerState::default()
                }),
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
            exclusions,
            partials,
            include_unexecuted,
        })
    }

    /// Stops tracing and atomically persists this worker's collected coverage.
    pub fn stop_and_save(self, py: Python<'_>) -> PyResult<()> {
        let Self {
            tracer,
            data_file,
            exclusions,
            partials,
            include_unexecuted,
        } = self;
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
            py.import("threading")?
                .call_method1("settrace", (py.None(),))?;
        }

        let borrowed = bound.borrow();
        let (executed, contexts, arcs, arc_contexts) = match borrowed.state.lock() {
            Ok(mut state) => (
                std::mem::take(&mut state.executed),
                std::mem::take(&mut state.contexts),
                std::mem::take(&mut state.arcs),
                std::mem::take(&mut state.arc_contexts),
            ),
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                (
                    std::mem::take(&mut state.executed),
                    std::mem::take(&mut state.contexts),
                    std::mem::take(&mut state.arcs),
                    std::mem::take(&mut state.arc_contexts),
                )
            }
        };
        let roots = borrowed.roots.clone();
        let branches = borrowed.branches;
        drop(borrowed);
        save_data(
            &data_file,
            CollectedCoverage {
                executed: into_owned_paths(executed),
                contexts: into_owned_paths(contexts),
                arcs: into_owned_paths(arcs),
                arc_contexts: into_owned_paths(arc_contexts),
            },
            branches,
            &roots,
            &exclusions,
            &partials,
            include_unexecuted,
        )
        .map_err(|err| {
            pyo3::exceptions::PyOSError::new_err(format!(
                "failed to write coverage data to {data_file}: {err}"
            ))
        })?;
        Ok(())
    }

    /// Returns canonical source roots for descendant-process collection.
    pub fn source_roots(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        self.tracer
            .bind(py)
            .borrow()
            .roots
            .iter()
            .map(|root| {
                root.to_str().map(ToOwned::to_owned).ok_or_else(|| {
                    PyValueError::new_err(format!(
                        "coverage source contains non-Unicode characters: `{}`",
                        root.display()
                    ))
                })
            })
            .collect()
    }

    /// Starts first-attempt setup before fixture-derived test identity is available.
    pub fn begin_pending_test_setup(&self, py: Python<'_>) {
        self.tracer.bind(py).borrow().begin_pending_test_setup();
    }

    /// Reattributes first-attempt setup after fixture-derived test identity is available.
    pub fn resolve_pending_test_setup(&self, py: Python<'_>, test: &str) {
        self.tracer
            .bind(py)
            .borrow()
            .resolve_pending_test_setup(test);
    }

    /// Attributes subsequent observations to one test lifecycle phase.
    pub fn set_test_context(&self, py: Python<'_>, test: &str, phase: CoveragePhase) {
        self.tracer.bind(py).borrow().set_test_context(test, phase);
    }

    /// Restores attribution for execution outside a concrete test lifecycle.
    pub fn clear_test_context(&self, py: Python<'_>) {
        self.tracer.bind(py).borrow().clear_test_context();
    }
}

#[derive(Default)]
struct TracerState {
    /// Files with the set of executed line numbers.
    executed: HashMap<TrackedPath, HashSet<u32>>,
    /// Per-line test contexts for files with executed lines.
    contexts: HashMap<TrackedPath, HashMap<u32, HashSet<String>>>,
    /// Line-to-line arcs executed in each file.
    arcs: HashMap<TrackedPath, HashSet<BranchArc>>,
    /// Per-arc test contexts for files with executed arcs.
    arc_contexts: HashMap<TrackedPath, HashMap<BranchArc, HashSet<String>>>,
    /// Current test context, if `--cov-context=test` is active and a test is running.
    current_context: Option<String>,
    /// Temporary setup context awaiting fixture-derived test identity.
    pending_context: Option<String>,
    /// Lines observed while [`Self::pending_context`] is active.
    pending_lines: HashMap<TrackedPath, HashSet<u32>>,
    /// Branches observed while [`Self::pending_context`] is active.
    pending_arcs: HashMap<TrackedPath, HashSet<BranchArc>>,
    /// Memoized result of [`compute_tracked_path`] per filename string.
    track_cache: HashMap<String, Option<TrackedPath>>,
    /// Memoized result of [`compute_tracked_path`] per live Python code object.
    code_cache: HashMap<usize, TrackedCode>,
    /// Last executed line per live Python code object for `sys.monitoring` arcs.
    monitoring_last_lines: HashMap<usize, u32>,
    /// Last executed line per traced frame for `sys.settrace` arcs.
    frame_last_lines: HashMap<usize, u32>,
}

/// Cached metadata retained with its Python code object to prevent pointer reuse bugs.
struct TrackedCode {
    code: Py<PyAny>,
    info: Option<TrackedCodeInfo>,
}

type TrackedPath = Arc<Path>;

#[derive(Clone)]
/// Mapping from a bytecode offset interval to its Python source line.
struct CodeLineRange {
    start: u32,
    end: u32,
    line: Option<u32>,
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
    contexts: bool,
    test_contexts: bool,
    static_context: Option<String>,
    branches: bool,
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
    /// tracked file, then returns `sys.monitoring.DISABLE` for normal coverage
    /// so the interpreter never calls us back for the same `(code, line)` pair.
    /// Context coverage keeps callbacks active so later tests can be attributed.
    fn line_cb(
        &self,
        py: Python<'_>,
        code: &Bound<'_, PyAny>,
        lineno: u32,
    ) -> PyResult<Option<Py<PyAny>>> {
        if let Some(info) = self.tracked_code_info(code)? {
            self.record_monitoring_line(code.as_ptr() as usize, info.path, info.first_line, lineno);
        }
        if self.contexts || self.branches {
            Ok(None)
        } else {
            Ok(self.monitoring_disable.get().map(|d| d.clone_ref(py)))
        }
    }

    fn branch_cb(
        &self,
        py: Python<'_>,
        code: &Bound<'_, PyAny>,
        offset: u32,
        destination: u32,
    ) -> PyResult<Option<Py<PyAny>>> {
        if let Some(info) = self.tracked_code_info(code)?
            && let Some(from) = line_for_offset(&info.line_ranges, offset)
        {
            let to = line_for_offset(&info.line_ranges, destination)
                .map(line_to_i32)
                .unwrap_or_else(|| -info.first_line);
            self.record_arc(
                info.path,
                BranchArc {
                    from: line_to_i32(from),
                    to,
                },
            );
        }
        if self.contexts {
            Ok(None)
        } else {
            Ok(self.monitoring_disable.get().map(|d| d.clone_ref(py)))
        }
    }

    fn return_cb(
        &self,
        code: &Bound<'_, PyAny>,
        _offset: u32,
        _value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        if let Some(info) = self.tracked_code_info(code)? {
            self.record_monitoring_return(code.as_ptr() as usize, info.path, info.first_line);
        }
        Ok(())
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
            let code = frame.getattr("f_code")?;
            let filename: String = code.getattr("co_filename")?.extract()?;
            let path = slf.borrow().tracked_path(&filename);
            if let Some(path) = path {
                let lineno: u32 = frame.getattr("f_lineno")?.extract()?;
                let first_line: i32 = code.getattr("co_firstlineno")?.extract()?;
                slf.borrow()
                    .record_frame_line(frame.as_ptr() as usize, path, first_line, lineno);
            }
        } else if event == "return" {
            let code = frame.getattr("f_code")?;
            let filename: String = code.getattr("co_filename")?.extract()?;
            let path = slf.borrow().tracked_path(&filename);
            if let Some(path) = path {
                let first_line: i32 = code.getattr("co_firstlineno")?.extract()?;
                slf.borrow()
                    .record_frame_return(frame.as_ptr() as usize, path, first_line);
            }
        }
        Ok(slf.getattr("local_trace")?.unbind())
    }
}

impl CoverageTracer {
    fn begin_pending_test_setup(&self) {
        if !self.test_contexts {
            return;
        }
        if let Ok(mut state) = self.state.lock() {
            let pending = compose_context(self.static_context.as_deref(), &[PENDING_SETUP_CONTEXT]);
            state.current_context.clone_from(&pending);
            state.pending_context = pending;
            state.pending_lines.clear();
            state.pending_arcs.clear();
        }
    }

    fn resolve_pending_test_setup(&self, test: &str) {
        if !self.test_contexts {
            return;
        }
        let Some(resolved) = compose_context(
            self.static_context.as_deref(),
            &[test, CoveragePhase::Setup.as_str()],
        ) else {
            return;
        };
        if let Ok(mut state) = self.state.lock() {
            let Some(pending) = state.pending_context.take() else {
                return;
            };
            let pending_lines = std::mem::take(&mut state.pending_lines);
            let pending_arcs = std::mem::take(&mut state.pending_arcs);
            replace_pending_context(&mut state.contexts, pending_lines, &pending, &resolved);
            replace_pending_context(&mut state.arc_contexts, pending_arcs, &pending, &resolved);
            state.current_context = Some(resolved);
        }
    }

    fn set_test_context(&self, test: &str, phase: CoveragePhase) {
        if !self.test_contexts {
            return;
        }
        if let Ok(mut state) = self.state.lock() {
            state.pending_context = None;
            state.pending_lines.clear();
            state.pending_arcs.clear();
            state.current_context =
                compose_context(self.static_context.as_deref(), &[test, phase.as_str()]);
        }
    }

    fn clear_test_context(&self) {
        if !self.test_contexts {
            return;
        }
        if let Ok(mut state) = self.state.lock() {
            state.pending_context = None;
            state.pending_lines.clear();
            state.pending_arcs.clear();
            state.current_context =
                compose_context(self.static_context.as_deref(), &[SESSION_CONTEXT]);
        }
    }

    fn record_monitoring_line(
        &self,
        code_id: usize,
        path: TrackedPath,
        first_line: i32,
        lineno: u32,
    ) {
        if let Ok(mut state) = self.state.lock() {
            if self.branches {
                let arc = state
                    .monitoring_last_lines
                    .insert(code_id, lineno)
                    .map_or_else(
                        || BranchArc {
                            from: -first_line,
                            to: line_to_i32(lineno),
                        },
                        |from| BranchArc {
                            from: line_to_i32(from),
                            to: line_to_i32(lineno),
                        },
                    );
                record_arc_in_state(&mut state, self.contexts, path.clone(), arc);
            }
            record_line_in_state(&mut state, self.contexts, path, lineno);
        }
    }

    fn record_monitoring_return(&self, code_id: usize, path: TrackedPath, first_line: i32) {
        if !self.branches {
            return;
        }
        if let Ok(mut state) = self.state.lock()
            && let Some(from) = state.monitoring_last_lines.remove(&code_id)
        {
            record_arc_in_state(
                &mut state,
                self.contexts,
                path,
                BranchArc {
                    from: line_to_i32(from),
                    to: -first_line,
                },
            );
        }
    }

    fn record_frame_line(&self, frame_id: usize, path: TrackedPath, first_line: i32, lineno: u32) {
        if let Ok(mut state) = self.state.lock() {
            if self.branches {
                let arc = state.frame_last_lines.insert(frame_id, lineno).map_or_else(
                    || BranchArc {
                        from: -first_line,
                        to: line_to_i32(lineno),
                    },
                    |from| BranchArc {
                        from: line_to_i32(from),
                        to: line_to_i32(lineno),
                    },
                );
                record_arc_in_state(&mut state, self.contexts, path.clone(), arc);
            }
            record_line_in_state(&mut state, self.contexts, path, lineno);
        }
    }

    fn record_frame_return(&self, frame_id: usize, path: TrackedPath, first_line: i32) {
        if !self.branches {
            return;
        }
        if let Ok(mut state) = self.state.lock()
            && let Some(from) = state.frame_last_lines.remove(&frame_id)
        {
            record_arc_in_state(
                &mut state,
                self.contexts,
                path,
                BranchArc {
                    from: line_to_i32(from),
                    to: -first_line,
                },
            );
        }
    }

    fn record_arc(&self, path: TrackedPath, arc: BranchArc) {
        if !self.branches || arc.from == arc.to {
            return;
        }
        if let Ok(mut state) = self.state.lock() {
            record_arc_in_state(&mut state, self.contexts, path, arc);
        }
    }

    /// Resolve a live Python code object without extracting `co_filename`
    /// after the first line callback for that object.
    fn tracked_code_info(&self, code: &Bound<'_, PyAny>) -> PyResult<Option<TrackedCodeInfo>> {
        let code_id = code.as_ptr() as usize;
        if let Ok(state) = self.state.lock()
            && let Some(cached) = state.code_cache.get(&code_id)
        {
            debug_assert!(cached.code.is(code));
            return Ok(cached.info.clone());
        }

        let filename: String = code.getattr("co_filename")?.extract()?;
        let info = if let Some(path) = self.tracked_path(&filename) {
            Some(TrackedCodeInfo {
                path,
                first_line: code.getattr("co_firstlineno")?.extract()?,
                line_ranges: code_line_ranges(code)?.into(),
            })
        } else {
            None
        };

        if let Ok(mut state) = self.state.lock() {
            state.code_cache.insert(
                code_id,
                TrackedCode {
                    code: code.clone().unbind(),
                    info: info.clone(),
                },
            );
        }

        Ok(info)
    }

    /// Resolve `filename` against the source roots. Returns the canonical
    /// path if the file should be tracked, or `None` otherwise. Memoized
    /// per filename string.
    fn tracked_path(&self, filename: &str) -> Option<TrackedPath> {
        if let Ok(state) = self.state.lock()
            && let Some(cached) = state.track_cache.get(filename)
        {
            return cached.clone();
        }
        let resolved = compute_tracked_path(filename, &self.roots)
            .map(|path| TrackedPath::from(path.into_boxed_path()));
        if let Ok(mut state) = self.state.lock() {
            state
                .track_cache
                .insert(filename.to_string(), resolved.clone());
        }
        resolved
    }
}

fn replace_pending_context<K: Copy + Eq + std::hash::Hash>(
    contexts: &mut HashMap<TrackedPath, HashMap<K, HashSet<String>>>,
    pending: HashMap<TrackedPath, HashSet<K>>,
    old: &str,
    new: &str,
) {
    for (path, keys) in pending {
        let Some(observations) = contexts.get_mut(&path) else {
            continue;
        };
        for key in keys {
            if let Some(values) = observations.get_mut(&key)
                && values.remove(old)
            {
                values.insert(new.to_owned());
            }
        }
    }
}

#[derive(Clone)]
struct TrackedCodeInfo {
    path: TrackedPath,
    first_line: i32,
    line_ranges: Arc<[CodeLineRange]>,
}

fn record_line_in_state(
    state: &mut TracerState,
    contexts_enabled: bool,
    path: TrackedPath,
    lineno: u32,
) {
    if contexts_enabled && let Some(context) = state.current_context.clone() {
        if state.pending_context.is_some() {
            state
                .pending_lines
                .entry(path.clone())
                .or_default()
                .insert(lineno);
        }
        state
            .executed
            .entry(path.clone())
            .or_default()
            .insert(lineno);
        state
            .contexts
            .entry(path)
            .or_default()
            .entry(lineno)
            .or_default()
            .insert(context);
    } else {
        state.executed.entry(path).or_default().insert(lineno);
    }
}

fn record_arc_in_state(
    state: &mut TracerState,
    contexts_enabled: bool,
    path: TrackedPath,
    arc: BranchArc,
) {
    if arc.from == arc.to {
        return;
    }
    if contexts_enabled && let Some(context) = state.current_context.clone() {
        if state.pending_context.is_some() {
            state
                .pending_arcs
                .entry(path.clone())
                .or_default()
                .insert(arc);
        }
        state.arcs.entry(path.clone()).or_default().insert(arc);
        state
            .arc_contexts
            .entry(path)
            .or_default()
            .entry(arc)
            .or_default()
            .insert(context);
    } else {
        state.arcs.entry(path).or_default().insert(arc);
    }
}

fn code_line_ranges(code: &Bound<'_, PyAny>) -> PyResult<Vec<CodeLineRange>> {
    let mut ranges = Vec::new();
    let co_lines = code.call_method0("co_lines")?;
    for item in co_lines.try_iter()? {
        let (start, end, line): (u32, u32, Option<u32>) = item?.extract()?;
        ranges.push(CodeLineRange { start, end, line });
    }
    Ok(ranges)
}

fn line_for_offset(ranges: &[CodeLineRange], offset: u32) -> Option<u32> {
    ranges
        .iter()
        .find(|range| range.start <= offset && offset < range.end)
        .and_then(|range| range.line)
}

fn line_to_i32(line: u32) -> i32 {
    i32::try_from(line).unwrap_or(i32::MAX)
}

fn compute_tracked_path(filename: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    if filename.is_empty() || filename.starts_with('<') {
        return None;
    }
    let canonical = fs::canonicalize(filename).ok()?;
    for root in roots {
        if canonical == *root {
            return Some(canonical);
        }
        let Ok(relative) = canonical.strip_prefix(root) else {
            continue;
        };
        if !relative
            .components()
            .any(|component| PATH_EXCLUDES.contains(&component.as_os_str().to_str().unwrap_or("")))
        {
            return Some(canonical);
        }
    }
    None
}

fn resolve_source_roots(
    py: Python<'_>,
    cwd: &Utf8Path,
    sources: &[String],
) -> PyResult<Vec<PathBuf>> {
    let importlib = py.import("importlib.util")?;
    let mut roots = BTreeSet::new();

    for source in sources {
        let candidate = if source.is_empty() {
            cwd.to_path_buf()
        } else {
            cwd.join(source)
        };
        if candidate.exists() {
            roots.insert(fs::canonicalize(candidate)?);
            continue;
        }

        let spec = importlib.call_method1("find_spec", (source,)).map_err(|error| {
            PyValueError::new_err(format!(
                "coverage source `{source}` was not found as path `{candidate}` and import lookup failed: {error}"
            ))
        })?;
        if spec.is_none() {
            return Err(PyValueError::new_err(format!(
                "coverage source `{source}` was not found as path `{candidate}` or as an importable module"
            )));
        }

        let locations = spec.getattr("submodule_search_locations")?;
        if !locations.is_none() {
            for location in locations.try_iter()? {
                let location: String = location?.extract()?;
                roots.insert(fs::canonicalize(&location).map_err(|error| {
                    PyValueError::new_err(format!(
                        "failed to resolve imported coverage source `{source}` at `{location}`: {error}"
                    ))
                })?);
            }
            continue;
        }

        let origin: Option<String> = spec.getattr("origin")?.extract()?;
        let Some(origin) = origin else {
            return Err(PyValueError::new_err(format!(
                "importable coverage source `{source}` has no Python source file"
            )));
        };
        let origin_path = PathBuf::from(&origin);
        if !is_python_source(&origin_path) {
            return Err(PyValueError::new_err(format!(
                "importable coverage source `{source}` has no Python source file (origin: `{origin}`)"
            )));
        }
        roots.insert(fs::canonicalize(&origin_path).map_err(|error| {
            PyValueError::new_err(format!(
                "failed to resolve imported coverage source `{source}` at `{origin}`: {error}"
            ))
        })?);
    }

    Ok(roots.into_iter().collect())
}

fn py_version_at_least(py: Python<'_>, major: u8, minor: u8) -> PyResult<bool> {
    let info = py.import("sys")?.getattr("version_info")?;
    let actual_major: u8 = info.get_item(0)?.extract()?;
    let actual_minor: u8 = info.get_item(1)?.extract()?;
    Ok((actual_major, actual_minor) >= (major, minor))
}

fn install_monitoring(py: Python<'_>, tracer: &Py<CoverageTracer>) -> PyResult<()> {
    let mon = py.import("sys")?.getattr("monitoring")?;
    let events = mon.getattr("events")?;
    let line_event = events.getattr("LINE")?;
    let line_event_value: u32 = line_event.extract()?;
    let disable = mon.getattr("DISABLE")?.unbind();

    let tool_id = (0u8..6u8)
        .find(|id| mon.call_method1("use_tool_id", (*id, "karva")).is_ok())
        .ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err(
                "no free sys.monitoring tool id available for coverage",
            )
        })?;

    let install_result = (|| -> PyResult<()> {
        let tracer_bound = tracer.bind(py);
        let callback = tracer_bound.getattr("line_cb")?;
        mon.call_method1("register_callback", (tool_id, &line_event, callback))?;

        let mut event_mask = line_event_value;
        if tracer_bound.borrow().branches {
            let branch_callback = tracer_bound.getattr("branch_cb")?;
            for event in branch_events(&events)? {
                mon.call_method1("register_callback", (tool_id, event, &branch_callback))?;
                event_mask |= event;
            }
            let return_callback = tracer_bound.getattr("return_cb")?;
            for event_name in ["PY_RETURN", "PY_UNWIND"] {
                let event: u32 = events.getattr(event_name)?.extract()?;
                mon.call_method1("register_callback", (tool_id, event, &return_callback))?;
                event_mask |= event;
            }
        }

        mon.call_method1("set_events", (tool_id, event_mask))?;
        {
            let bound = tracer_bound.borrow();
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

fn branch_events(events: &Bound<'_, PyAny>) -> PyResult<Vec<u32>> {
    let left = events.getattr("BRANCH_LEFT");
    let right = events.getattr("BRANCH_RIGHT");
    if let (Ok(left), Ok(right)) = (left, right) {
        return Ok(vec![left.extract()?, right.extract()?]);
    }
    Ok(vec![events.getattr("BRANCH")?.extract()?])
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
    py.import("sys")?.call_method1("settrace", (&trace,))?;
    py.import("threading")?.call_method1("settrace", (trace,))?;
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

fn into_owned_paths<V>(map: HashMap<TrackedPath, V>) -> HashMap<PathBuf, V> {
    map.into_iter()
        .map(|(path, value)| (path.to_path_buf(), value))
        .collect()
}

struct CollectedCoverage {
    executed: HashMap<PathBuf, HashSet<u32>>,
    contexts: HashMap<PathBuf, HashMap<u32, HashSet<String>>>,
    arcs: HashMap<PathBuf, HashSet<BranchArc>>,
    arc_contexts: HashMap<PathBuf, HashMap<BranchArc, HashSet<String>>>,
}

fn save_data(
    data_file: &Utf8Path,
    collected: CollectedCoverage,
    branches: bool,
    roots: &[PathBuf],
    exclusions: &CoverageExclusions,
    partials: &CoveragePartials,
    include_unexecuted: bool,
) -> std::io::Result<()> {
    let CollectedCoverage {
        mut executed,
        mut contexts,
        mut arcs,
        mut arc_contexts,
    } = collected;
    if include_unexecuted {
        for path in walk_source_files(roots) {
            executed.entry(path).or_default();
        }
    }

    let mut files = BTreeMap::new();
    for (path, hits) in executed {
        let (executable, excluded) = executable_lines_with_exclusions(&path, exclusions)?;
        if executable.is_empty() {
            continue;
        }
        let mut executed_lines: Vec<u32> = hits.intersection(&executable).copied().collect();
        executed_lines.sort_unstable();
        let mut executable_lines_vec: Vec<u32> = executable.into_iter().collect();
        executable_lines_vec.sort_unstable();
        let context_lines = contexts
            .remove(&path)
            .unwrap_or_default()
            .into_iter()
            .filter(|(line, _)| executed_lines.binary_search(line).is_ok())
            .map(|(line, contexts)| (line, contexts.into_iter().collect::<BTreeSet<_>>()))
            .collect();
        let branches = if branches {
            let (possible, partial) = branch_analysis_with_exclusions(&path, exclusions, partials)?;
            let executed_arcs = arcs.remove(&path).unwrap_or_default();
            let mut possible_vec: Vec<BranchArc> = possible.iter().copied().collect();
            possible_vec.sort_unstable();
            let mut executed_vec: Vec<BranchArc> = executed_arcs.iter().copied().collect();
            executed_vec.sort_unstable();
            let contexts = arc_contexts
                .remove(&path)
                .unwrap_or_default()
                .into_iter()
                .filter(|(arc, _)| executed_arcs.contains(arc))
                .map(|(arc, contexts)| BranchContextEntry {
                    arc,
                    contexts: contexts.into_iter().collect(),
                })
                .collect();
            Some(BranchEntry {
                possible: possible_vec,
                executed: executed_vec,
                contexts,
                partial: partial.into_iter().collect(),
            })
        } else {
            None
        };
        files.insert(
            path.to_string_lossy().into_owned(),
            FileEntry {
                executable: executable_lines_vec,
                excluded: {
                    let mut lines: Vec<_> = excluded.into_iter().collect();
                    lines.sort_unstable();
                    lines
                },
                executed: executed_lines,
                contexts: context_lines,
                branches,
            },
        );
    }

    let parent = data_file
        .parent()
        .filter(|parent| !parent.as_str().is_empty())
        .unwrap_or_else(|| Utf8Path::new("."));
    fs::create_dir_all(parent.as_std_path())?;
    let source_roots = roots
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect();
    let bytes = serde_json::to_vec(&WorkerFile {
        source_roots,
        files,
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent.as_std_path())?;
    temporary.write_all(&bytes)?;
    temporary
        .persist(data_file.as_std_path())
        .map(|_| ())
        .map_err(|error| error.error)
}

#[cfg(test)]
mod tests {
    use pyo3::ffi::c_str;
    use pyo3::types::PyDict;

    use super::*;

    #[test]
    fn source_path_takes_precedence_over_import_lookup() {
        let directory = tempfile::tempdir().expect("temp directory");
        let cwd = Utf8Path::from_path(directory.path()).expect("UTF-8 path");
        fs::create_dir(cwd.join("ambiguous")).expect("source directory");

        Python::initialize();
        let roots = Python::attach(|py| resolve_source_roots(py, cwd, &["ambiguous".to_string()]))
            .expect("resolved source");

        assert_eq!(
            roots,
            vec![fs::canonicalize(cwd.join("ambiguous")).expect("canonical source")]
        );
    }

    #[test]
    fn sourceless_import_is_rejected() {
        let directory = tempfile::tempdir().expect("temp directory");
        let cwd = Utf8Path::from_path(directory.path()).expect("UTF-8 path");

        Python::initialize();
        let error = Python::attach(|py| resolve_source_roots(py, cwd, &["sys".to_string()]))
            .expect_err("built-in module has no source");

        assert!(
            error
                .to_string()
                .contains("importable coverage source `sys` has no Python source file")
        );
    }

    #[test]
    fn record_arc_attributes_the_current_context() {
        let path: TrackedPath = Arc::from(PathBuf::from("module.py").into_boxed_path());
        let arc = BranchArc { from: 1, to: 2 };
        let mut state = TracerState {
            current_context: Some("test_module::test_value".to_string()),
            ..TracerState::default()
        };

        record_arc_in_state(&mut state, true, path.clone(), arc);

        assert_eq!(state.arcs.get(&path), Some(&HashSet::from([arc])));
        assert_eq!(
            state
                .arc_contexts
                .get(&path)
                .and_then(|arcs| arcs.get(&arc)),
            Some(&HashSet::from(["test_module::test_value".to_string()]))
        );
    }

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
                contexts: false,
                test_contexts: false,
                static_context: None,
                branches: false,
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
        self.filename_calls = 0
        self.first_line_calls = 0
        self.lines_calls = 0

    @property
    def co_filename(self):
        self.filename_calls += 1
        if self.filename_calls > 1:
            raise AssertionError("co_filename should be cached")
        return filename

    @property
    def co_firstlineno(self):
        self.first_line_calls += 1
        if self.first_line_calls > 1:
            raise AssertionError("co_firstlineno should be cached")
        return 1

    def co_lines(self):
        self.lines_calls += 1
        if self.lines_calls > 1:
            raise AssertionError("co_lines should be cached")
        return iter([(0, 2, 1)])

code = Code()
"#
                ),
                Some(&locals),
                Some(&locals),
            )?;
            let code = locals.get_item("code")?.expect("code object");

            assert_eq!(
                tracer
                    .tracked_code_info(&code)?
                    .map(|info| info.path.to_path_buf()),
                expected
            );
            assert_eq!(
                tracer
                    .tracked_code_info(&code)?
                    .map(|info| info.path.to_path_buf()),
                expected
            );

            let calls = (
                code.getattr("filename_calls")?.extract::<u32>()?,
                code.getattr("first_line_calls")?.extract::<u32>()?,
                code.getattr("lines_calls")?.extract::<u32>()?,
            );
            assert_eq!(calls, (1, 1, 1));

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
    fn untracked_code_skips_line_metadata() {
        Python::initialize();
        Python::attach(|py| -> PyResult<()> {
            let tracer = CoverageTracer {
                roots: Vec::new(),
                contexts: false,
                test_contexts: false,
                static_context: None,
                branches: false,
                state: Mutex::new(TracerState::default()),
                monitoring_tool_id: OnceLock::new(),
                monitoring_disable: OnceLock::new(),
            };
            let locals = PyDict::new(py);
            py.run(
                c_str!(
                    r#"
class Code:
    co_filename = "<generated>"

    @property
    def co_firstlineno(self):
        raise AssertionError("untracked code should skip co_firstlineno")

    def co_lines(self):
        raise AssertionError("untracked code should skip co_lines")

code = Code()
"#
                ),
                Some(&locals),
                Some(&locals),
            )?;
            let code = locals.get_item("code")?.expect("code object");

            assert!(tracer.tracked_code_info(&code)?.is_none());
            assert!(tracer.tracked_code_info(&code)?.is_none());

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

        let err = save_data(
            &data_file,
            CollectedCoverage {
                executed,
                contexts: HashMap::new(),
                arcs: HashMap::new(),
                arc_contexts: HashMap::new(),
            },
            false,
            &[],
            &CoverageExclusions::default(),
            &CoveragePartials::default(),
            true,
        )
        .expect_err("missing source should fail");

        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(err.to_string().contains("missing.py"), "{err}");
        assert!(!data_file.exists());
    }
}
