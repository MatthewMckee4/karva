use std::cell::{Cell, RefCell};
use std::rc::Rc;

use karva_diagnostic::Diagnostic;
use pyo3::exceptions::{PyRuntimeError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::PyIterator;
use thiserror::Error;

use crate::diagnostic::invalid_fixture_finalizer_diagnostic;
use crate::discovery::models::definition::FunctionDefinition;
use crate::utils::run_coroutine;

/// Invalid state observed while resuming a generator fixture for teardown.
#[derive(Debug, Error)]
enum FinalizerError {
    /// Generator produced another value instead of completing.
    #[error("Fixture had more than one yield statement")]
    MultipleYields,

    /// Generator could not be resumed or raised during teardown.
    #[error("Failed to reset fixture: {0}")]
    ResetFailed(PyErr),
}

/// Lives as long as the test or fixture that registered these callbacks.
#[derive(Debug, Default)]
pub struct FinalizerCallbacks {
    callbacks: RefCell<Vec<Py<PyAny>>>,
    closed: Cell<bool>,
}

impl FinalizerCallbacks {
    pub(crate) fn is_active(&self) -> bool {
        !self.closed.get()
    }

    pub(crate) fn register(&self, py: Python<'_>, callback: Py<PyAny>) -> PyResult<()> {
        if self.closed.get() {
            return Err(PyRuntimeError::new_err("fixture request has ended"));
        }
        if !callback.bind(py).is_callable() {
            return Err(PyTypeError::new_err("finalizer must be callable"));
        }
        self.callbacks.borrow_mut().push(callback);
        Ok(())
    }

    pub(crate) fn run(&self, py: Python<'_>, mut report: impl FnMut(&PyErr)) {
        loop {
            let callback = self.callbacks.borrow_mut().pop();
            let Some(callback) = callback else {
                self.closed.set(true);
                break;
            };
            if let Err(error) = callback.call0(py) {
                report(&error);
            }
        }
    }
}

/// Represents the teardown portion of a generator fixture.
///
/// When a fixture yields a value, the code after the yield runs as cleanup.
/// This struct holds the generator iterator to resume for teardown.
///
/// ```python
/// @fixture
/// def my_fixture():
///     # setup
///     yield value
///     # teardown (finalizer runs this part)
/// ```
#[derive(Debug)]
pub struct Finalizer {
    /// The generator or async generator, positioned after yield, ready for teardown.
    pub(crate) generator: Option<Py<PyAny>>,

    /// Callbacks belonging to this fixture instance, including those added during teardown.
    pub(crate) callbacks: Option<Rc<FinalizerCallbacks>>,

    /// Whether this finalizer wraps an async generator (requires `asyncio.run()`).
    pub(crate) is_async: bool,

    /// Immutable fixture identity, syntax, and source.
    pub(crate) definition: Rc<FunctionDefinition>,
}

impl Finalizer {
    /// Runs generator teardown and every registered callback, preserving all failures.
    pub(crate) fn run(self, py: Python<'_>) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        if let Some(generator) = &self.generator {
            let result = if self.is_async {
                Self::run_async_teardown(py, generator)
            } else {
                Self::run_sync_teardown(py, generator)
            };
            if let Err(error) = result {
                let (reason, error) = match &error {
                    FinalizerError::MultipleYields => (error.to_string(), None),
                    FinalizerError::ResetFailed(error) => {
                        ("Failed to reset fixture".to_string(), Some(error))
                    }
                };
                diagnostics.push(self.diagnostic(py, &reason, error));
            }
        }
        if let Some(callbacks) = &self.callbacks {
            callbacks.run(py, |error| {
                diagnostics.push(self.diagnostic(py, "Fixture finalizer failed", Some(error)));
            });
        }
        diagnostics
    }

    fn diagnostic(&self, py: Python<'_>, reason: &str, error: Option<&PyErr>) -> Diagnostic {
        invalid_fixture_finalizer_diagnostic(
            py,
            self.definition.source_file().clone(),
            self.definition.statement(),
            reason,
            error,
        )
    }

    /// Runs teardown for a sync generator fixture.
    fn run_sync_teardown(py: Python<'_>, generator: &Py<PyAny>) -> Result<(), FinalizerError> {
        let mut generator = generator
            .clone_ref(py)
            .into_bound(py)
            .cast_into::<PyIterator>()
            .map_err(|error| FinalizerError::ResetFailed(error.into()))?;
        match generator.next() {
            None => Ok(()),
            Some(Ok(_)) => Err(FinalizerError::MultipleYields),
            Some(Err(error)) => Err(FinalizerError::ResetFailed(error)),
        }
    }

    /// Runs teardown for an async generator fixture.
    fn run_async_teardown(py: Python<'_>, generator: &Py<PyAny>) -> Result<(), FinalizerError> {
        let coroutine = generator
            .bind(py)
            .call_method0("__anext__")
            .map_err(FinalizerError::ResetFailed)?;
        match run_coroutine(py, coroutine.unbind()) {
            Ok(_) => Err(FinalizerError::MultipleYields),
            Err(error) => {
                if error.is_instance_of::<pyo3::exceptions::PyStopAsyncIteration>(py) {
                    Ok(())
                } else {
                    Err(FinalizerError::ResetFailed(error))
                }
            }
        }
    }
}
