use pyo3::{prelude::*, types::PyIterator};

use crate::{Location, diagnostic::Diagnostic, extensions::fixtures::FixtureScope};

/// Represents a generator function that can be used to run the finalizer section of a fixture.
///
/// ```py
/// def fixture():
///     yield
///     # Finalizer logic here
/// ```
#[derive(Debug, Clone)]
pub struct Finalizer {
    pub(crate) location: Option<Location>,
    pub(crate) fixture_return: Py<PyIterator>,
    pub(crate) scope: FixtureScope,
}

impl Finalizer {
    pub(crate) const fn scope(&self) -> FixtureScope {
        self.scope
    }

    pub(crate) fn run(self, py: Python<'_>) -> Option<Diagnostic> {
        let mut generator = self.fixture_return.bind(py).clone();
        match generator.next()? {
            Ok(_) => Some(Diagnostic::warning(
                self.location,
                "Fixture had more than one yield statement",
            )),
            Err(err) => Some(Diagnostic::warning(
                self.location,
                &format!("Failed to reset fixture: {}", err.value(py)),
            )),
        }
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
        warnings:

        warning: Fixture <test>.test_file::fixture_generator had more than one yield statement

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

        assert_snapshot!(result.display(), @r"
        warnings:

        warning: Failed to reset fixture <test>.test_file::fixture_generator
        fixture-error

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]
        ");
    }
}
