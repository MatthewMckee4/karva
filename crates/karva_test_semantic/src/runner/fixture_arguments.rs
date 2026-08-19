//! Python keyword arguments prepared from fixtures and parametrization.

use std::collections::HashMap;

use pyo3::prelude::*;
use pyo3::types::PyDict;
use ruff_python_ast::Parameters;

/// Keyword arguments resolved for a test or fixture call.
#[derive(Default)]
pub struct FixtureArguments {
    /// Python values keyed by test or fixture parameter name.
    inner: HashMap<String, Py<PyAny>>,
}

impl FixtureArguments {
    /// Owns borrowed fixture names only when a call needs diagnostics.
    pub(super) fn from_fixture_values(arguments: Vec<(&str, Py<PyAny>)>) -> Self {
        Self {
            inner: arguments
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect(),
        }
    }

    /// Inserts one named Python argument.
    pub fn insert(&mut self, name: String, value: Py<PyAny>) -> Option<Py<PyAny>> {
        self.inner.insert(name, value)
    }

    /// Returns whether no arguments were prepared.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Iterates arguments in unspecified hash-map order.
    pub fn iter(&self) -> std::collections::hash_map::Iter<'_, String, Py<PyAny>> {
        self.inner.iter()
    }

    /// Iterates arguments by Python signature position, then name.
    ///
    /// Without a function signature, arguments are ordered by name.
    pub fn iter_in_signature_order<'a>(
        &'a self,
        parameters: Option<&Parameters>,
    ) -> impl Iterator<Item = (&'a String, &'a Py<PyAny>)> {
        let mut arguments = self.iter().collect::<Vec<_>>();
        arguments.sort_by(|(left, _), (right, _)| {
            let left_position = parameters
                .and_then(|parameters| parameters.index(left))
                .unwrap_or(usize::MAX);
            let right_position = parameters
                .and_then(|parameters| parameters.index(right))
                .unwrap_or(usize::MAX);
            left_position
                .cmp(&right_position)
                .then_with(|| left.cmp(right))
        });
        arguments.into_iter()
    }

    /// Builds a Python keyword-argument dictionary.
    pub fn to_kwargs<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let kwargs = PyDict::new(py);
        for (key, value) in self {
            kwargs.set_item(key, value)?;
        }
        Ok(kwargs)
    }
}

impl<'a> IntoIterator for &'a FixtureArguments {
    type IntoIter = std::collections::hash_map::Iter<'a, String, Py<PyAny>>;
    type Item = (&'a String, &'a Py<PyAny>);

    fn into_iter(self) -> Self::IntoIter {
        self.inner.iter()
    }
}

#[cfg(test)]
mod tests {
    use pyo3::IntoPyObjectExt;

    use super::*;

    #[test]
    fn builds_python_kwargs() {
        Python::initialize();
        Python::attach(|py| {
            let mut arguments = FixtureArguments::default();
            arguments.insert(
                "answer".to_string(),
                42i32.into_py_any(py).expect("convert int"),
            );

            let kwargs = arguments.to_kwargs(py).expect("build kwargs");
            let answer = kwargs
                .get_item("answer")
                .expect("lookup should succeed")
                .expect("answer should exist")
                .extract::<i32>()
                .expect("answer should be an int");

            assert_eq!(answer, 42);
        });
    }
}
