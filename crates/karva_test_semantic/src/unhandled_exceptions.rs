use std::sync::{Mutex, MutexGuard, PoisonError};

use pyo3::exceptions::PySystemExit;
use pyo3::prelude::*;

#[derive(Debug)]
pub enum UnhandledExceptionKind {
    Thread {
        name: String,
    },
    Unraisable {
        context: Option<String>,
        object: String,
    },
}

#[derive(Debug)]
pub struct UnhandledExceptionEvent {
    pub(crate) test_name: Option<String>,
    pub(crate) kind: UnhandledExceptionKind,
    pub(crate) error: PyErr,
}

#[derive(Default)]
struct HookState {
    active_test: Option<String>,
    events: Vec<UnhandledExceptionEvent>,
}

#[pyclass(frozen)]
struct ExceptionHooks {
    state: Mutex<HookState>,
}

impl ExceptionHooks {
    fn state(&self) -> MutexGuard<'_, HookState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set_active_test(&self, test_name: Option<&str>) {
        self.state().active_test = test_name.map(String::from);
    }

    fn take_events_for(&self, test_name: &str) -> Vec<UnhandledExceptionEvent> {
        let mut state = self.state();
        let events = std::mem::take(&mut state.events);
        let (matching, remaining) = events
            .into_iter()
            .partition(|event| event.test_name.as_deref() == Some(test_name));
        state.events = remaining;
        matching
    }

    fn take_all_events(&self) -> Vec<UnhandledExceptionEvent> {
        std::mem::take(&mut self.state().events)
    }

    fn push(&self, kind: UnhandledExceptionKind, error: PyErr) {
        let mut state = self.state();
        let test_name = state.active_test.clone();
        state.events.push(UnhandledExceptionEvent {
            test_name,
            kind,
            error,
        });
    }
}

#[pymethods]
impl ExceptionHooks {
    fn thread_excepthook(&self, args: &Bound<'_, PyAny>) -> PyResult<()> {
        let exception = args.getattr("exc_value")?;
        if exception.is_instance_of::<PySystemExit>() {
            return Ok(());
        }

        let thread_name = args
            .getattr("thread")
            .and_then(|thread| thread.getattr("name"))
            .and_then(|name| name.extract::<String>())
            .unwrap_or_else(|_| "<unknown>".to_string());
        self.push(
            UnhandledExceptionKind::Thread { name: thread_name },
            PyErr::from_value(exception),
        );
        Ok(())
    }

    fn unraisablehook(&self, args: &Bound<'_, PyAny>) -> PyResult<()> {
        let exception = args.getattr("exc_value")?;
        let context = args.getattr("err_msg")?.extract::<Option<String>>()?;
        let object = args.getattr("object")?;
        let object = object
            .getattr("__qualname__")
            .and_then(|name| name.extract::<String>())
            .or_else(|_| object.repr().and_then(|repr| repr.extract::<String>()))
            .unwrap_or_else(|_| "<unrepresentable object>".to_string());
        self.push(
            UnhandledExceptionKind::Unraisable { context, object },
            PyErr::from_value(exception),
        );
        Ok(())
    }
}

pub struct UnhandledExceptionCapture {
    hooks: Py<ExceptionHooks>,
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

        threading.setattr("excepthook", hooks.bind(py).getattr("thread_excepthook")?)?;
        if let Err(error) = sys.setattr("unraisablehook", hooks.bind(py).getattr("unraisablehook")?)
        {
            if let Err(restore_error) =
                threading.setattr("excepthook", previous_thread_hook.bind(py))
            {
                tracing::warn!(
                    "failed to restore threading.excepthook after hook setup error: {restore_error}"
                );
            }
            return Err(error);
        }

        Ok(Self {
            hooks,
            previous_thread_hook,
            previous_unraisable_hook,
        })
    }

    pub(crate) fn set_active_test(&self, test_name: Option<&str>) {
        self.hooks.get().set_active_test(test_name);
    }

    pub(crate) fn take_events_for(&self, test_name: &str) -> Vec<UnhandledExceptionEvent> {
        self.hooks.get().take_events_for(test_name)
    }

    pub(crate) fn finish(self, py: Python<'_>) -> PyResult<Vec<UnhandledExceptionEvent>> {
        self.set_active_test(None);
        let threading = py.import("threading")?;
        let sys = py.import("sys")?;
        let thread_result = threading.setattr("excepthook", self.previous_thread_hook.bind(py));
        let unraisable_result =
            sys.setattr("unraisablehook", self.previous_unraisable_hook.bind(py));
        let events = self.hooks.get().take_all_events();
        thread_result?;
        unraisable_result?;
        Ok(events)
    }
}
