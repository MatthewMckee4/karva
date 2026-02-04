use std::rc::Rc;

use karva_python_semantic::QualifiedFunctionName;
use pyo3::prelude::*;
use pyo3::types::PyIterator;
use ruff_python_ast::StmtFunctionDef;

use crate::Context;
use crate::diagnostic::report_invalid_fixture_finalizer;
use crate::extensions::fixtures::FixtureScope;
use crate::utils::source_file;

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
    /// The generator iterator, positioned after yield, ready for teardown.
    pub(crate) fixture_return: Py<PyIterator>,

    /// The scope determines when this finalizer runs.
    pub(crate) scope: FixtureScope,

    /// Optional name of the fixture for error reporting.
    pub(crate) fixture_name: Option<QualifiedFunctionName>,

    /// Optional AST definition for error reporting.
    pub(crate) stmt_function_def: Option<Rc<StmtFunctionDef>>,
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
                source_file(context.system(), fixture_name.module_path().path()),
                &stmt_function_def,
                invalid_finalizer_reason,
            );
        }
    }
}
