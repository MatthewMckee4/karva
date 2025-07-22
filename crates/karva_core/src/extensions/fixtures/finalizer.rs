use pyo3::prelude::*;

use crate::diagnostic::Diagnostic;

#[derive(Debug, Default)]
pub(crate) struct Finalizers {
    finalizers: Vec<Finalizer>,
}

impl Finalizers {
    #[must_use]
    pub(crate) const fn new(finalizers: Vec<Finalizer>) -> Self {
        Self { finalizers }
    }

    pub(crate) fn update(&mut self, other: Self) {
        self.finalizers.extend(other.finalizers);
    }

    #[must_use]
    pub(crate) fn run(&self, py: Python<'_>) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        for finalizer in &self.finalizers {
            if let Some(diagnostic) = finalizer.run(py) {
                diagnostics.push(diagnostic);
            }
        }
        diagnostics
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Finalizer {
    fixture_name: String,
    fixture_return: Py<PyAny>,
}

impl Finalizer {
    #[must_use]
    pub(crate) const fn new(fixture_name: String, fixture_return: Py<PyAny>) -> Self {
        Self {
            fixture_name,
            fixture_return,
        }
    }

    #[must_use]
    pub(crate) fn run(&self, py: Python<'_>) -> Option<Diagnostic> {
        match self.fixture_return.call_method0(py, "__next__") {
            Ok(_) => Some(Diagnostic::warning(
                "fixture-error",
                Some(format!(
                    "Fixture {} had more than one yield statement",
                    self.fixture_name
                )),
                None,
            )),
            Err(e) => {
                if e.is_instance_of::<pyo3::exceptions::PyStopIteration>(py) {
                    None
                } else {
                    Some(Diagnostic::warning(
                        "fixture-error",
                        Some(format!("Failed to reset fixture {}", self.fixture_name)),
                        None,
                    ))
                }
            }
        }
    }
}
