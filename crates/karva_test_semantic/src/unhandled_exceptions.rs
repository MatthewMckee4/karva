//! Worker-wide hooks for exceptions that cannot propagate through a Python call.
//!
//! One worker executes one test attempt at a time. Events observed during that
//! attempt belong to it; discovery and broader-scope cleanup events belong to
//! the run. Python objects are never inspected while holding the state lock.

use std::sync::{Mutex, MutexGuard, PoisonError};

use pyo3::exceptions::PySystemExit;
use pyo3::prelude::*;
use pyo3::types::{PyFunction, PyTraceback, PyType};

/// Origin details retained without keeping a thread or unraisable object alive.
pub enum UnhandledExceptionKind {
    /// An exception escaping `Thread.run`, with its thread's display name.
    Thread { name: String },

    /// A failure reported by Python's unraisable hook.
    Unraisable {
        /// Optional explanation supplied by Python.
        context: Option<String>,

        /// Callable name or object type, obtained without running user `repr`.
        object: String,
    },
}

/// A captured exception, including its traceback until diagnostic rendering.
pub struct UnhandledExceptionEvent {
    pub(crate) kind: UnhandledExceptionKind,

    pub(crate) error: PyErr,
}

#[derive(Default)]
struct HookState {
    /// Present only while a concrete attempt's setup, call, or teardown runs.
    attempt_events: Option<Vec<UnhandledExceptionEvent>>,
    run_events: Vec<UnhandledExceptionEvent>,
}

#[pyclass(frozen)]
struct ExceptionHooks {
    state: Mutex<HookState>,
}

impl ExceptionHooks {
    fn state(&self) -> MutexGuard<'_, HookState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push(&self, kind: UnhandledExceptionKind, error: PyErr) {
        let mut state = self.state();
        let event = UnhandledExceptionEvent { kind, error };
        if let Some(events) = &mut state.attempt_events {
            events.push(event);
        } else {
            state.run_events.push(event);
        }
    }
}

#[pymethods]
impl ExceptionHooks {
    fn thread_excepthook(&self, args: &Bound<'_, PyAny>) -> PyResult<()> {
        let error = hook_error(args)?;
        if error.is_instance_of::<PySystemExit>(args.py()) {
            return Ok(());
        }
        let name = args
            .getattr("thread")
            .and_then(|thread| thread.getattr("name"))
            .and_then(|name| name.extract::<String>())
            .unwrap_or_else(|_| "<unknown>".to_string());
        self.push(UnhandledExceptionKind::Thread { name }, error);
        Ok(())
    }

    fn unraisablehook(&self, args: &Bound<'_, PyAny>) -> PyResult<()> {
        let error = hook_error(args)?;
        let context = args
            .getattr("err_msg")
            .and_then(|message| message.extract::<Option<String>>())
            .unwrap_or_default()
            .filter(|message| !message.is_empty());
        let object = args.getattr("object")?;
        // Calling arbitrary __repr__ here can raise more unraisable exceptions
        // recursively. Function names and type names require no user callbacks.
        let object = if object.is_none() {
            "None".to_string()
        } else if let Ok(function) = object.cast::<PyFunction>() {
            function
                .getattr("__qualname__")
                .and_then(|name| name.extract::<String>())
                .unwrap_or_else(|_| "<function>".to_string())
        } else {
            object.get_type().name().map_or_else(
                |_| "<unrepresentable object>".to_string(),
                |name| format!("<{name} object>"),
            )
        };
        self.push(
            UnhandledExceptionKind::Unraisable { context, object },
            error,
        );
        Ok(())
    }
}

/// Preserves the hook's explicit traceback, even when no exception value exists.
fn hook_error(args: &Bound<'_, PyAny>) -> PyResult<PyErr> {
    let value = args.getattr("exc_value")?;
    let error = if value.is_none() {
        let exception_type = args.getattr("exc_type")?.cast_into::<PyType>()?;
        PyErr::from_type(exception_type, ())
    } else {
        PyErr::from_value(value)
    };
    let traceback = args.getattr("exc_traceback")?;
    let traceback = if traceback.is_none() {
        None
    } else {
        Some(traceback.cast_into::<PyTraceback>()?)
    };
    error.set_traceback(args.py(), traceback);
    Ok(error)
}

/// Owns installed hooks and restores both originals when the run finishes.
///
/// Installation is mandatory: failing to install a hook must fail the run,
/// rather than allowing background exceptions to leave tests green.
pub struct UnhandledExceptionCapture {
    hooks: Py<ExceptionHooks>,
    threading: Py<PyAny>,
    sys: Py<PyAny>,
    previous_thread_hook: Py<PyAny>,
    previous_unraisable_hook: Py<PyAny>,
}

impl UnhandledExceptionCapture {
    pub(crate) fn start(py: Python<'_>) -> PyResult<Self> {
        let threading = py.import("threading")?;
        let sys = py.import("sys")?;
        let previous_thread_hook = threading.getattr("excepthook")?.unbind();
        let previous_unraisable_hook = sys.getattr("unraisablehook")?.unbind();
        let hooks = Py::new(
            py,
            ExceptionHooks {
                state: Mutex::new(HookState::default()),
            },
        )?;
        let thread_hook = hooks.bind(py).getattr("thread_excepthook")?;
        let unraisable_hook = hooks.bind(py).getattr("unraisablehook")?;
        threading.setattr("excepthook", thread_hook)?;
        if let Err(error) = sys.setattr("unraisablehook", unraisable_hook) {
            threading.setattr("excepthook", previous_thread_hook.bind(py))?;
            return Err(error);
        }
        Ok(Self {
            hooks,
            threading: threading.into_any().unbind(),
            sys: sys.into_any().unbind(),
            previous_thread_hook,
            previous_unraisable_hook,
        })
    }

    /// Begins attribution before fixture setup, without needing a resolved name.
    pub(crate) fn begin_attempt(&self) {
        self.hooks.get().state().attempt_events = Some(Vec::new());
    }

    /// Ends attribution atomically with taking all events for this attempt.
    pub(crate) fn finish_attempt(&self) -> Vec<UnhandledExceptionEvent> {
        self.hooks
            .get()
            .state()
            .attempt_events
            .take()
            .unwrap_or_default()
    }

    /// Restores both hooks, retaining captured events even if restoration fails.
    pub(crate) fn finish(self, py: Python<'_>) -> (Vec<UnhandledExceptionEvent>, Vec<PyErr>) {
        let thread_result = self
            .threading
            .bind(py)
            .setattr("excepthook", self.previous_thread_hook.bind(py));
        let unraisable_result = self
            .sys
            .bind(py)
            .setattr("unraisablehook", self.previous_unraisable_hook.bind(py));
        let mut state = self.hooks.get().state();
        let mut events = std::mem::take(&mut state.run_events);
        events.extend(state.attempt_events.take().into_iter().flatten());
        let errors = thread_result
            .err()
            .into_iter()
            .chain(unraisable_result.err())
            .collect();
        (events, errors)
    }
}

#[cfg(test)]
mod tests {
    use pyo3::types::PyModule;

    use super::*;

    #[test]
    fn hooks_restore_originals_and_accept_missing_exception_values() {
        Python::initialize();
        Python::attach(|py| {
            let threading = py.import("threading").expect("threading");
            let sys = py.import("sys").expect("sys");
            let thread_hook = threading.getattr("excepthook").expect("thread hook");
            let unraisable_hook = sys.getattr("unraisablehook").expect("unraisable hook");
            let capture = UnhandledExceptionCapture::start(py).expect("install hooks");
            capture.begin_attempt();
            let module = PyModule::from_code(
                py,
                c"
from types import SimpleNamespace

args = SimpleNamespace(
    exc_type=ValueError,
    exc_value=None,
    exc_traceback=None,
    thread=None,
)
",
                c"hook_test.py",
                c"hook_test",
            )
            .expect("hook args");
            threading
                .getattr("excepthook")
                .expect("installed hook")
                .call1((module.getattr("args").expect("args"),))
                .expect("capture without value");
            let args = module.getattr("args").expect("args");
            args.setattr("err_msg", "").expect("empty context");
            args.setattr("object", py.None()).expect("missing object");
            sys.getattr("unraisablehook")
                .expect("installed unraisable hook")
                .call1((args,))
                .expect("capture without context or object");
            let events = capture.finish_attempt();
            let (run_events, errors) = capture.finish(py);
            assert!(
                threading
                    .getattr("excepthook")
                    .expect("restored hook")
                    .is(&thread_hook)
            );
            assert!(
                sys.getattr("unraisablehook")
                    .expect("restored hook")
                    .is(&unraisable_hook)
            );
            assert!(errors.is_empty());
            assert!(run_events.is_empty());
            assert_eq!(events.len(), 2);
            assert!(
                matches!(&events[1].kind, UnhandledExceptionKind::Unraisable { context: None, object } if object == "None")
            );
            assert!(
                events[0]
                    .error
                    .is_instance_of::<pyo3::exceptions::PyValueError>(py)
            );
        });
    }
}
