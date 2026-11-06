use std::sync::Arc;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

/// Represents a custom tag/marker that stores arbitrary metadata.
///
/// This allows users to create their own markers with custom names, args, and kwargs.
#[derive(Debug, Clone)]
pub struct CustomTag {
    name: String,
    args: Vec<Arc<Py<PyAny>>>,
    kwargs: Vec<(String, Arc<Py<PyAny>>)>,
}

impl CustomTag {
    /// Copies benchmark keyword controls; positional metadata is unsupported.
    pub(super) fn benchmark_options<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        if !self.args.is_empty() {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "benchmark tag accepts keyword options only",
            ));
        }
        let options = PyDict::new(py);
        for (key, value) in &self.kwargs {
            options.set_item(key, value.bind(py))?;
        }
        Ok(options)
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn new(
        name: String,
        args: Vec<Arc<Py<PyAny>>>,
        kwargs: Vec<(String, Arc<Py<PyAny>>)>,
    ) -> Self {
        Self { name, args, kwargs }
    }

    /// Try to create a `CustomTag` from a pytest mark.
    pub(super) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let name = py_mark.getattr("name").ok()?.extract::<String>().ok()?;

        let args = if let Ok(args_tuple) = py_mark.getattr("args")
            && let Ok(tuple) = args_tuple.cast::<PyTuple>()
        {
            tuple.iter().map(|item| Arc::new(item.unbind())).collect()
        } else {
            Vec::new()
        };

        let kwargs = if let Ok(kwargs_dict) = py_mark.getattr("kwargs")
            && let Ok(dict) = kwargs_dict.cast::<PyDict>()
        {
            dict.iter()
                .filter_map(|(key, value)| {
                    let key_str = key.extract::<String>().ok()?;
                    Some((key_str, Arc::new(value.unbind())))
                })
                .collect()
        } else {
            Vec::new()
        };

        Some(Self::new(name, args, kwargs))
    }
}
