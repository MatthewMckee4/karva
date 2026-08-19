//! Python keyword arguments prepared from fixtures and parametrization.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use pyo3::prelude::*;
use pyo3::types::PyDict;
use ruff_python_ast::Parameters;

/// Keyword arguments resolved for a test or fixture call.
#[derive(Default)]
pub struct FixtureArguments {
    /// Python values keyed by test or fixture parameter name.
    inner: HashMap<ArgumentName, Py<PyAny>>,
}

enum ArgumentName {
    Owned(String),
    Shared(Rc<str>),
}

impl ArgumentName {
    fn as_str(&self) -> &str {
        match self {
            Self::Owned(name) => name,
            Self::Shared(name) => name,
        }
    }
}

impl PartialEq for ArgumentName {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for ArgumentName {}

impl Hash for ArgumentName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl FixtureArguments {
    /// Inserts one named Python argument.
    pub fn insert(&mut self, name: String, value: Py<PyAny>) -> Option<Py<PyAny>> {
        self.inner.insert(ArgumentName::Owned(name), value)
    }

    /// Inserts an argument using a fixture definition's shared name.
    pub(super) fn insert_shared(&mut self, name: &Rc<str>, value: Py<PyAny>) -> Option<Py<PyAny>> {
        self.inner
            .insert(ArgumentName::Shared(Rc::clone(name)), value)
    }

    /// Returns whether no arguments were prepared.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Iterates arguments in unspecified hash-map order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Py<PyAny>)> {
        self.inner
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }

    /// Iterates arguments by Python signature position, then name.
    ///
    /// Without a function signature, arguments are ordered by name.
    pub fn iter_in_signature_order<'a>(
        &'a self,
        parameters: Option<&Parameters>,
    ) -> impl Iterator<Item = (&'a str, &'a Py<PyAny>)> {
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
        for (key, value) in self.iter() {
            kwargs.set_item(key, value)?;
        }
        Ok(kwargs)
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

    #[test]
    fn builds_python_kwargs_from_shared_names() {
        Python::initialize();
        Python::attach(|py| {
            let mut arguments = FixtureArguments::default();
            let name = Rc::from("answer");
            arguments.insert_shared(&name, 42i32.into_py_any(py).expect("convert shared value"));

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

    #[test]
    fn owned_names_replace_matching_shared_names() {
        Python::initialize();
        Python::attach(|py| {
            let mut arguments = FixtureArguments::default();
            let name = Rc::from("answer");
            arguments.insert_shared(&name, 41i32.into_py_any(py).expect("convert shared value"));

            let replaced = arguments
                .insert(
                    "answer".to_string(),
                    42i32.into_py_any(py).expect("convert owned value"),
                )
                .expect("matching shared name should be replaced");

            assert_eq!(replaced.extract::<i32>(py).expect("extract value"), 41);
        });
    }
}
