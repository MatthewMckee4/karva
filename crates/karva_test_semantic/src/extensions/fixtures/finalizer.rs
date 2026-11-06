use std::rc::Rc;
use std::time::Instant;

use camino::Utf8PathBuf;
use karva_diagnostic::{Diagnostic, LifecycleTiming, Reporter, TimingPhase};
use pyo3::prelude::*;
use pyo3::types::PyIterator;
use thiserror::Error;

use crate::diagnostic::invalid_fixture_finalizer_diagnostic;
use crate::discovery::models::definition::FunctionDefinition;
use crate::extensions::fixtures::FixtureScope;
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
    pub(crate) fixture_return: Py<PyAny>,

    /// Whether this finalizer wraps an async generator (requires `asyncio.run()`).
    pub(crate) is_async: bool,

    /// The scope determines when this finalizer runs.
    pub(crate) scope: FixtureScope,

    /// Defining package for package-scoped teardown.
    pub(crate) package_owner: Utf8PathBuf,

    /// Immutable fixture identity, syntax, and source.
    pub(crate) definition: Rc<FunctionDefinition>,
}

impl Finalizer {
    /// Resumes teardown once and reports invalid generator behavior.
    pub(crate) fn run(self, py: Python<'_>, reporter: &dyn Reporter) -> Result<(), Diagnostic> {
        let started = reporter.collect_timings().then(Instant::now);
        let result = if self.is_async {
            self.run_async_teardown(py)
        } else {
            self.run_sync_teardown(py)
        };

        if let Some(started) = started {
            let mut timing = LifecycleTiming::new(
                self.definition.name().to_string(),
                self.definition.source_file(),
                self.definition.statement().name.range,
                TimingPhase::FixtureTeardown,
                started.elapsed(),
            );
            timing.scope = Some(self.scope.name().to_owned());
            reporter.report_timing(timing);
        }
        result.map_err(|error| {
            let (reason, error) = match &error {
                FinalizerError::MultipleYields => (error.to_string(), None),
                FinalizerError::ResetFailed(error) => {
                    ("Failed to reset fixture".to_string(), Some(error))
                }
            };
            invalid_fixture_finalizer_diagnostic(
                py,
                self.definition.source_file().clone(),
                self.definition.statement(),
                &reason,
                error,
            )
        })
    }

    /// Runs teardown for a sync generator fixture.
    fn run_sync_teardown(&self, py: Python<'_>) -> Result<(), FinalizerError> {
        let mut generator = self
            .fixture_return
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
    fn run_async_teardown(&self, py: Python<'_>) -> Result<(), FinalizerError> {
        let coroutine = self
            .fixture_return
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
