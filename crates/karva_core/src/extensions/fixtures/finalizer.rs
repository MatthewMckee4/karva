use pyo3::{prelude::*, types::PyIterator};
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

use crate::{
    Context, diagnostic::report_invalid_fixture_finalizer, extensions::fixtures::FixtureScope,
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
    pub(crate) source_file: SourceFile,
    pub(crate) stmt_function_def: StmtFunctionDef,
}

impl Finalizer {
    pub(crate) const fn scope(&self) -> FixtureScope {
        self.scope
    }

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

        report_invalid_fixture_finalizer(
            context,
            self.source_file,
            &self.stmt_function_def,
            invalid_finalizer_reason,
        );
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use karva_test::TestContext;

    use crate::TestRunner;

    #[test]
    fn test_fixture_generator_two_yields() {
        let test_context = TestContext::with_file(
            "<test>/test_file.py",
            r"
import karva

@karva.fixture
def fixture_generator():
    yield 1
    yield 2

def test_fixture_generator(fixture_generator):
    assert fixture_generator == 1
    ",
        );

        let result = test_context.test();

        assert_snapshot!(result.display(), @r"
        diagnostics:
        invalid-fixture-finalizer: Discovered an invalid fixture finalizer `fixture_generator`
         --> <test>/test_file.py:5:5
          |
        4 | @karva.fixture
        5 | def fixture_generator():
          |     ^^^^^^^^^^^^^^^^^
        6 |     yield 1
        7 |     yield 2
          |
        info: Reason: Fixture had more than one yield statement

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]
        ");
    }

    #[test]
    fn test_fixture_generator_fail_in_teardown() {
        let test_context = TestContext::with_file(
            "<test>/test_file.py",
            r#"
import karva

@karva.fixture
def fixture_generator():
    yield 1
    raise ValueError("fixture-error")

def test_fixture_generator(fixture_generator):
    assert fixture_generator == 1
    "#,
        );

        let result = test_context.test();

        assert_snapshot!(result.display(), @r#"
        diagnostics:
        invalid-fixture-finalizer: Discovered an invalid fixture finalizer `fixture_generator`
         --> <test>/test_file.py:5:5
          |
        4 | @karva.fixture
        5 | def fixture_generator():
          |     ^^^^^^^^^^^^^^^^^
        6 |     yield 1
        7 |     raise ValueError("fixture-error")
          |
        info: Reason: Failed to reset fixture: fixture-error

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]
        "#);
    }
}
