use std::rc::Rc;

use pyo3::prelude::*;
use pyo3::types::PyIterator;
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

use crate::Context;
use crate::diagnostic::report_invalid_fixture_finalizer;
use crate::extensions::fixtures::FixtureScope;
use crate::utils::run_coroutine;

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

    /// Optional AST definition for error reporting.
    pub(crate) stmt_function_def: Option<Rc<StmtFunctionDef>>,

    /// Optional source code for error reporting.
    pub(crate) source_file: Option<SourceFile>,
}

impl Finalizer {
    pub(crate) fn run(self, context: &Context, py: Python<'_>) {
        let invalid_finalizer_reason = if self.is_async {
            self.run_async_teardown(py)
        } else {
            self.run_sync_teardown(py)
        };

        if let Some(reason) = invalid_finalizer_reason
            && let Some(stmt_function_def) = self.stmt_function_def
            && let Some(source_file) = self.source_file
        {
            report_invalid_fixture_finalizer(context, source_file, &stmt_function_def, &reason);
        }
    }

    /// Runs teardown for a sync generator fixture.
    fn run_sync_teardown(&self, py: Python<'_>) -> Option<String> {
        let Ok(mut generator) = self
            .fixture_return
            .clone_ref(py)
            .into_bound(py)
            .cast_into::<PyIterator>()
        else {
            return None;
        };
        let generator_next_result = generator.next()?;
        let reason = match generator_next_result {
            Ok(_) => "Fixture had more than one yield statement".to_string(),
            Err(err) => format!("Failed to reset fixture: {}", err.value(py)),
        };
        Some(reason)
    }

    /// Runs teardown for an async generator fixture.
    fn run_async_teardown(&self, py: Python<'_>) -> Option<String> {
        let bound = self.fixture_return.bind(py);
        let anext_result = match bound.call_method0("__anext__") {
            Ok(coroutine) => run_coroutine(py, coroutine.unbind()),
            Err(_) => return None,
        };
        let reason = match anext_result {
            Ok(_) => "Fixture had more than one yield statement".to_string(),
            Err(err) => {
                if err.is_instance_of::<pyo3::exceptions::PyStopAsyncIteration>(py) {
                    return None;
                }
                format!("Failed to reset fixture: {}", err.value(py))
            }
        };
        Some(reason)
    }
}
