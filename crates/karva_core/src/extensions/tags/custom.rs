use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

/// Represents a custom tag/marker that stores arbitrary metadata.
///
/// This allows users to create their own markers with custom names, args, and kwargs.
#[derive(Debug, Clone)]
pub struct CustomTag {
    name: String,
    args: Vec<Py<PyAny>>,
    kwargs: Vec<(String, Py<PyAny>)>,
}

impl CustomTag {
    pub(crate) fn new(
        name: String,
        args: Vec<Py<PyAny>>,
        kwargs: Vec<(String, Py<PyAny>)>,
    ) -> Self {
        Self { name, args, kwargs }
    }

    /// Convert this tag to a `PyTag` for use in Python.
    pub(crate) fn to_py_tag(&self) -> super::python::PyTag {
        super::python::PyTag::Custom {
            tag_name: self.name.clone(),
            tag_args: self.args.clone(),
            tag_kwargs: self.kwargs.clone(),
        }
    }

    /// Try to create a `CustomTag` from a pytest mark.
    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let name = py_mark.getattr("name").ok()?.extract::<String>().ok()?;

        // Extract args
        let args = if let Ok(args_tuple) = py_mark.getattr("args") {
            if let Ok(tuple) = args_tuple.cast::<PyTuple>() {
                tuple.iter().map(pyo3::Bound::unbind).collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Extract kwargs
        let kwargs = if let Ok(kwargs_dict) = py_mark.getattr("kwargs") {
            if let Ok(dict) = kwargs_dict.cast::<PyDict>() {
                dict.iter()
                    .filter_map(|(key, value)| {
                        let key_str = key.extract::<String>().ok()?;
                        Some((key_str, value.unbind()))
                    })
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        Some(Self::new(name, args, kwargs))
    }
}
