use pyo3::prelude::*;

use crate::diagnostic::Diagnostic;

#[derive(Debug)]
pub struct Finalizer {
    fixture_name: String,
    fixture_return: Py<PyAny>,
}

impl Finalizer {
    #[must_use]
    pub const fn new(fixture_name: String, fixture_return: Py<PyAny>) -> Self {
        Self {
            fixture_name,
            fixture_return,
        }
    }

    #[must_use]
    pub fn reset(&self, py: Python<'_>) -> Option<Diagnostic> {
        match self.fixture_return.call_method0(py, "__next__") {
            Ok(_) => Some(Diagnostic::warning(
                "fixture-error",
                format!(
                    "Fixture {} had more than one yield statement",
                    self.fixture_name
                ),
                None,
            )),
            Err(e) => {
                if e.is_instance_of::<pyo3::exceptions::PyStopIteration>(py) {
                    None
                } else {
                    Some(Diagnostic::warning(
                        "fixture-error",
                        format!("Failed to reset fixture {}", self.fixture_name),
                        None,
                    ))
                }
            }
        }
    }
}
