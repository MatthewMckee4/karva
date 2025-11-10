use pyo3::{prelude::*, types::PyIterator};

use crate::diagnostic::Diagnostic;

/// Represents a collection of finalizers.
#[derive(Debug, Default)]
pub(crate) struct Finalizers(Vec<Finalizer>);

impl Finalizers {
    pub(crate) const fn new(finalizers: Vec<Finalizer>) -> Self {
        Self(finalizers)
    }

    pub(crate) fn update(&mut self, other: Self) {
        self.0.extend(other.0);
    }

    pub(crate) fn run(&self, py: Python<'_>) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for finalizer in &self.0 {
            if let Some(diagnostic) = finalizer.run(py) {
                diagnostics.push(diagnostic);
            }
        }
        diagnostics
    }
}

/// Represents a generator function that can be used to run the finalizer section of a fixture.
///
/// ```py
/// def fixture():
///     yield
///     # Finalizer logic here
/// ```
#[derive(Debug, Clone)]
pub(crate) struct Finalizer {
    fixture_name: String,
    fixture_return: Py<PyIterator>,
}

impl Finalizer {
    pub(crate) const fn new(fixture_name: String, fixture_return: Py<PyIterator>) -> Self {
        Self {
            fixture_name,
            fixture_return,
        }
    }

    pub(crate) fn run(&self, py: Python<'_>) -> Option<Diagnostic> {
        let mut generator = self.fixture_return.bind(py).clone();
        match generator.next()? {
            Ok(_) => Some(Diagnostic::warning(&format!(
                "Fixture {} had more than one yield statement",
                self.fixture_name
            ))),
            Err(_) => Some(Diagnostic::warning(&format!(
                "Failed to reset fixture {}",
                self.fixture_name
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use karva_project::utils::module_name;
    use karva_test::TestContext;

    use crate::{TestResultStats, TestRunner, diagnostic::Diagnostic};

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

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        let module_name_path = test_context
            .mapped_path("<test>")
            .unwrap()
            .join("test_file.py");
        let module_name = module_name(&test_context.cwd(), &module_name_path).unwrap();

        assert_eq!(*result.stats(), expected_stats, "{result:?}");

        assert_eq!(result.diagnostics().len(), 1);
        let first_diagnostic = &result.diagnostics()[0];
        let expected_diagnostic = Diagnostic::warning(&format!(
            "Fixture {module_name}::fixture_generator had more than one yield statement"
        ));

        assert_eq!(*first_diagnostic, expected_diagnostic);
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

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        let module_name_path = test_context
            .mapped_path("<test>")
            .unwrap()
            .join("test_file.py");
        let module_name = module_name(&test_context.cwd(), &module_name_path).unwrap();

        assert_eq!(*result.stats(), expected_stats, "{result:?}");

        assert_eq!(result.diagnostics().len(), 1);
        let first_diagnostic = &result.diagnostics()[0];
        let expected_diagnotic = Diagnostic::warning(&format!(
            "Failed to reset fixture {module_name}::fixture_generator"
        ));

        assert_eq!(*first_diagnostic, expected_diagnotic);
    }
}
