use std::rc::Rc;

use camino::Utf8PathBuf;
use pyo3::prelude::*;
use pyo3::types::PyIterator;
use thiserror::Error;

use ruff_db::diagnostic::Diagnostic;

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
    ResetFailed(String),
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
    /// Python teardown operation retained until its owning scope completes.
    operation: FinalizerOperation,

    /// The scope determines when this finalizer runs.
    pub(crate) scope: FixtureScope,

    /// Fixture instance that owns this teardown, or `None` for test finalizers.
    pub(crate) fixture_name: Option<String>,

    /// Defining package for package-scoped teardown.
    pub(crate) package_owner: Utf8PathBuf,

    /// Immutable fixture identity, syntax, and source.
    pub(crate) definition: Rc<FunctionDefinition>,
}

#[derive(Debug)]
enum FinalizerOperation {
    Generator {
        fixture_return: Py<PyAny>,
        is_async: bool,
    },
    Callback(Py<PyAny>),
}

impl Finalizer {
    pub(crate) fn generator(
        fixture_return: Py<PyAny>,
        is_async: bool,
        fixture_name: String,
        scope: FixtureScope,
        package_owner: Utf8PathBuf,
        definition: Rc<FunctionDefinition>,
    ) -> Self {
        Self {
            operation: FinalizerOperation::Generator {
                fixture_return,
                is_async,
            },
            scope,
            fixture_name: Some(fixture_name),
            package_owner,
            definition,
        }
    }

    pub(crate) fn callback(
        callback: Py<PyAny>,
        fixture_name: Option<String>,
        scope: FixtureScope,
        package_owner: Utf8PathBuf,
        definition: Rc<FunctionDefinition>,
    ) -> Self {
        Self {
            operation: FinalizerOperation::Callback(callback),
            scope,
            fixture_name,
            package_owner,
            definition,
        }
    }

    /// Resumes teardown once and reports invalid generator behavior.
    pub(crate) fn run(self, py: Python<'_>) -> Result<(), Diagnostic> {
        self.execute(py).map_err(|error| {
            invalid_fixture_finalizer_diagnostic(
                self.definition.source_file().clone(),
                self.definition.statement(),
                &error.to_string(),
            )
        })
    }

    /// Runs teardown while replacing a parametrized fixture instance.
    pub(crate) fn run_for_replacement(self, py: Python<'_>) -> Result<(), String> {
        self.execute(py).map_err(|error| error.to_string())
    }

    fn execute(&self, py: Python<'_>) -> Result<(), FinalizerError> {
        match &self.operation {
            FinalizerOperation::Generator {
                fixture_return,
                is_async: true,
            } => Self::run_async_teardown(py, fixture_return),
            FinalizerOperation::Generator {
                fixture_return,
                is_async: false,
            } => Self::run_sync_teardown(py, fixture_return),
            FinalizerOperation::Callback(callback) => callback
                .call0(py)
                .map(|_| ())
                .map_err(|error| FinalizerError::ResetFailed(error.value(py).to_string())),
        }
    }

    /// Runs teardown for a sync generator fixture.
    fn run_sync_teardown(py: Python<'_>, fixture_return: &Py<PyAny>) -> Result<(), FinalizerError> {
        let mut generator = fixture_return
            .clone_ref(py)
            .into_bound(py)
            .cast_into::<PyIterator>()
            .map_err(|error| FinalizerError::ResetFailed(error.to_string()))?;
        match generator.next() {
            None => Ok(()),
            Some(Ok(_)) => Err(FinalizerError::MultipleYields),
            Some(Err(error)) => Err(FinalizerError::ResetFailed(error.value(py).to_string())),
        }
    }

    /// Runs teardown for an async generator fixture.
    fn run_async_teardown(
        py: Python<'_>,
        fixture_return: &Py<PyAny>,
    ) -> Result<(), FinalizerError> {
        let coroutine = fixture_return
            .bind(py)
            .call_method0("__anext__")
            .map_err(|error| FinalizerError::ResetFailed(error.value(py).to_string()))?;
        match run_coroutine(py, coroutine.unbind()) {
            Ok(_) => Err(FinalizerError::MultipleYields),
            Err(error) => {
                if error.is_instance_of::<pyo3::exceptions::PyStopAsyncIteration>(py) {
                    Ok(())
                } else {
                    Err(FinalizerError::ResetFailed(error.value(py).to_string()))
                }
            }
        }
    }
}
