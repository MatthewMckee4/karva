use std::rc::Rc;

use camino::Utf8PathBuf;
use pyo3::prelude::*;
use pyo3::types::PyIterator;
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;
use thiserror::Error;

use ruff_db::diagnostic::Diagnostic;

use crate::diagnostic::invalid_fixture_finalizer_diagnostic;
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
    /// The generator or async generator, positioned after yield, ready for teardown.
    pub(crate) fixture_return: Py<PyAny>,

    /// Whether this finalizer wraps an async generator (requires `asyncio.run()`).
    pub(crate) is_async: bool,

    /// The scope determines when this finalizer runs.
    pub(crate) scope: FixtureScope,

    /// Defining package for package-scoped teardown.
    pub(crate) package_owner: Utf8PathBuf,

    /// AST definition used to locate teardown errors.
    pub(crate) stmt_function_def: Rc<StmtFunctionDef>,

    /// Source code containing the fixture definition.
    pub(crate) source_file: SourceFile,
}

impl Finalizer {
    /// Resumes teardown once and reports invalid generator behavior.
    pub(crate) fn run(self, py: Python<'_>) -> Result<(), Diagnostic> {
        let result = if self.is_async {
            self.run_async_teardown(py)
        } else {
            self.run_sync_teardown(py)
        };

        result.map_err(|error| {
            invalid_fixture_finalizer_diagnostic(
                self.source_file,
                &self.stmt_function_def,
                &error.to_string(),
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
            .map_err(|error| FinalizerError::ResetFailed(error.to_string()))?;
        match generator.next() {
            None => Ok(()),
            Some(Ok(_)) => Err(FinalizerError::MultipleYields),
            Some(Err(error)) => Err(FinalizerError::ResetFailed(error.value(py).to_string())),
        }
    }

    /// Runs teardown for an async generator fixture.
    fn run_async_teardown(&self, py: Python<'_>) -> Result<(), FinalizerError> {
        let coroutine = self
            .fixture_return
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
