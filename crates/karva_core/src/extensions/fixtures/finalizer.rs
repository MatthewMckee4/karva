use std::sync::Arc;

use pyo3::{prelude::*, types::PyIterator};
use ruff_python_ast::StmtFunctionDef;

use crate::{
    Context, QualifiedFunctionName, diagnostic::report_invalid_fixture_finalizer,
    extensions::fixtures::FixtureScope, utils::source_file,
};

/// Represents a generator function that can be used to run the finalizer section of a fixture.
///
/// ```py
/// def fixture():
///     yield
///     # Finalizer logic here
/// ```
#[derive(Debug, Clone)]
pub struct Finalizer {
    pub(crate) fixture_return: Py<PyIterator>,
    pub(crate) scope: FixtureScope,
    pub(crate) fixture_name: Option<QualifiedFunctionName>,
    pub(crate) stmt_function_def: Option<Arc<StmtFunctionDef>>,
}

impl Finalizer {
    pub(crate) fn run(self, context: &Context, py: Python<'_>) {
        let mut generator = self.fixture_return.bind(py).clone();
        let Some(generator_next_result) = generator.next() else {
            // We do not care if the `next` function fails, this should not happen.
            return;
        };
        let invalid_finalizer_reason = match generator_next_result {
            Ok(_) => "Fixture had more than one yield statement",
            Err(err) => &format!("Failed to reset fixture: {}", err.value(py)),
        };

        if let Some(stmt_function_def) = self.stmt_function_def
            && let Some(fixture_name) = self.fixture_name
        {
            report_invalid_fixture_finalizer(
                context,
                source_file(context.db().system(), fixture_name.module_path().path()),
                &stmt_function_def,
                invalid_finalizer_reason,
            );
        }
    }
}
