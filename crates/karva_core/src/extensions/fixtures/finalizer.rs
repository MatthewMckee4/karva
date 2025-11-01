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
            Ok(_) => Some(Diagnostic::warning(
                "fixture-error",
                Some(format!(
                    "Fixture {} had more than one yield statement",
                    self.fixture_name
                )),
                None,
            )),
            Err(_) => Some(Diagnostic::warning(
                "fixture-error",
                Some(format!("Failed to reset fixture {}", self.fixture_name)),
                None,
            )),
        }
    }
}
