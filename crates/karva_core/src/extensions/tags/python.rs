use pyo3::IntoPyObjectExt;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

use crate::extensions::functions::python::Param;

#[derive(Debug, Clone)]
#[pyclass(name = "tag")]
pub enum PyTag {
    #[pyo3(name = "parametrize")]
    Parametrize {
        arg_names: Vec<String>,
        arg_values: Vec<Param>,
    },

    #[pyo3(name = "use_fixtures")]
    UseFixtures { fixture_names: Vec<String> },

    #[pyo3(name = "skip")]
    Skip {
        conditions: Vec<bool>,
        reason: Option<String>,
    },

    #[pyo3(name = "expect_fail")]
    ExpectFail {
        conditions: Vec<bool>,
        reason: Option<String>,
    },

    #[pyo3(name = "custom")]
    Custom {
        tag_name: String,
        tag_args: Vec<Py<PyAny>>,
        tag_kwargs: Vec<(String, Py<PyAny>)>,
    },
}

#[pymethods]
impl PyTag {
    #[getter]
    pub fn name(&self) -> String {
        match self {
            Self::Parametrize { .. } => "parametrize".to_string(),
            Self::UseFixtures { .. } => "use_fixtures".to_string(),
            Self::Skip { .. } => "skip".to_string(),
            Self::ExpectFail { .. } => "expect_fail".to_string(),
            Self::Custom { tag_name, .. } => tag_name.clone(),
        }
    }

    #[getter]
    pub fn args<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, pyo3::types::PyTuple>> {
        match self {
            Self::Custom { tag_args, .. } => {
                let py_args: Vec<Bound<'py, PyAny>> =
                    tag_args.iter().map(|arg| arg.bind(py).clone()).collect();
                pyo3::types::PyTuple::new(py, py_args)
            }
            _ => Ok(pyo3::types::PyTuple::empty(py)),
        }
    }

    #[getter]
    pub fn kwargs<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, pyo3::types::PyDict>> {
        match self {
            Self::Custom { tag_kwargs, .. } => {
                let py_dict = pyo3::types::PyDict::new(py);
                for (key, value) in tag_kwargs {
                    py_dict.set_item(key, value.bind(py))?;
                }
                Ok(py_dict)
            }
            _ => Ok(pyo3::types::PyDict::new(py)),
        }
    }
}

#[derive(Debug, Clone)]
#[pyclass(name = "Tags")]
pub struct PyTags {
    pub inner: Vec<PyTag>,
}

#[pymethods]
impl PyTags {
    #[pyo3(signature = (f, /))]
    pub fn __call__(&self, py: Python<'_>, f: Py<PyAny>) -> PyResult<Py<PyAny>> {
        if let Ok(tag_obj) = f.cast_bound::<Self>(py) {
            tag_obj.borrow_mut().inner.extend(self.inner.clone());
            return tag_obj.into_py_any(py);
        } else if let Ok(test_case) = f.cast_bound::<PyTestFunction>(py) {
            test_case.borrow_mut().tags.inner.extend(self.inner.clone());
            return test_case.into_py_any(py);
        } else if f.bind(py).is_callable() {
            let test_case = PyTestFunction {
                tags: self.clone(),
                function: f,
            };
            return test_case.into_py_any(py);
        } else if let Ok(tag) = f.extract::<PyTag>(py) {
            let mut new_tags = self.inner.clone();
            new_tags.push(tag);
            return new_tags.into_py_any(py);
        }
        Err(PyErr::new::<PyTypeError, _>(
            "Expected a Tags, TestCase, or Tag object",
        ))
    }
}

#[pymodule]
pub mod tags {
    use pyo3::IntoPyObjectExt;
    use pyo3::exceptions::PyTypeError;
    use pyo3::prelude::*;
    use pyo3::types::PyTuple;

    use super::{CustomTagBuilder, PyTag, PyTags};
    use crate::extensions::functions::python::Param;
    use crate::extensions::tags::parametrize::parse_parametrize_args;
    use crate::extensions::tags::python::PyTestFunction;

    /// Handle dynamic attribute access for custom tags.
    ///
    /// This allows users to create tags like `@karva.tags.slow` or `@karva.tags.integration`.
    #[pyfunction]
    fn __getattr__(py: Python<'_>, name: String) -> PyResult<Py<PyAny>> {
        CustomTagBuilder { tag_name: name }.into_py_any(py)
    }

    #[pyfunction]
    pub fn parametrize(
        arg_names: &Bound<'_, PyAny>,
        arg_values: &Bound<'_, PyAny>,
    ) -> PyResult<PyTags> {
        let Some((names, parametrization)) = parse_parametrize_args(arg_names, arg_values) else {
            return Err(PyErr::new::<PyTypeError, _>(
                "Expected a string or a list of strings for the arg_names, and a list of lists of objects for the arg_values",
            ));
        };

        Ok(PyTags {
            inner: vec![PyTag::Parametrize {
                arg_names: names,
                arg_values: parametrization
                    .into_iter()
                    .map(Param::from_parametrization)
                    .collect(),
            }],
        })
    }

    #[pyfunction]
    #[pyo3(signature = (*fixture_names))]
    pub fn use_fixtures(fixture_names: &Bound<'_, PyTuple>) -> PyResult<PyTags> {
        let mut names = Vec::new();
        for item in fixture_names.iter() {
            if let Ok(name) = item.extract::<String>() {
                names.push(name);
            } else {
                return Err(PyErr::new::<PyTypeError, _>(
                    "Expected a string or a list of strings for fixture names",
                ));
            }
        }
        Ok(PyTags {
            inner: vec![PyTag::UseFixtures {
                fixture_names: names,
            }],
        })
    }

    #[pyfunction]
    #[pyo3(signature = (*conditions, reason = None))]
    pub fn skip(
        py: Python<'_>,
        conditions: &Bound<'_, PyTuple>,
        reason: Option<String>,
    ) -> PyResult<Py<PyAny>> {
        let mut bool_conditions = Vec::new();

        // Check if the first argument is a function (decorator without parentheses)
        if conditions.len() == 1 {
            if let Ok(first_item) = conditions.get_item(0) {
                if first_item.is_callable() {
                    return PyTestFunction {
                        tags: PyTags {
                            inner: vec![PyTag::Skip {
                                conditions: vec![],
                                reason: None,
                            }],
                        },
                        function: first_item.unbind(),
                    }
                    .into_py_any(py);
                }
                // Check if the first argument is a string (reason passed as positional arg)
                if let Ok(reason_str) = first_item.extract::<String>() {
                    return PyTags {
                        inner: vec![PyTag::Skip {
                            conditions: vec![],
                            reason: Some(reason_str),
                        }],
                    }
                    .into_py_any(py);
                }
            }
        }

        // Parse boolean conditions from positional arguments
        for item in conditions.iter() {
            if let Ok(bool_val) = item.extract::<bool>() {
                bool_conditions.push(bool_val);
            } else {
                return Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(
                    "Expected boolean values for conditions",
                ));
            }
        }

        PyTags {
            inner: vec![PyTag::Skip {
                conditions: bool_conditions,
                reason,
            }],
        }
        .into_py_any(py)
    }

    #[pyfunction]
    #[pyo3(signature = (*conditions, reason = None))]
    pub fn expect_fail(
        py: Python<'_>,
        conditions: &Bound<'_, PyTuple>,
        reason: Option<String>,
    ) -> PyResult<Py<PyAny>> {
        let mut bool_conditions = Vec::new();

        // Check if the first argument is a function (decorator without parentheses)
        if conditions.len() == 1 {
            if let Ok(first_item) = conditions.get_item(0) {
                if first_item.is_callable() {
                    return PyTestFunction {
                        tags: PyTags {
                            inner: vec![PyTag::ExpectFail {
                                conditions: vec![],
                                reason: None,
                            }],
                        },
                        function: first_item.unbind(),
                    }
                    .into_py_any(py);
                }
                // Check if the first argument is a string (reason passed as positional arg)
                if let Ok(reason_str) = first_item.extract::<String>() {
                    return PyTags {
                        inner: vec![PyTag::ExpectFail {
                            conditions: vec![],
                            reason: Some(reason_str),
                        }],
                    }
                    .into_py_any(py);
                }
            }
        }

        // Parse boolean conditions from positional arguments
        for item in conditions.iter() {
            if let Ok(bool_val) = item.extract::<bool>() {
                bool_conditions.push(bool_val);
            } else {
                return Err(PyErr::new::<pyo3::exceptions::PyTypeError, _>(
                    "Expected boolean values for conditions",
                ));
            }
        }

        PyTags {
            inner: vec![PyTag::ExpectFail {
                conditions: bool_conditions,
                reason,
            }],
        }
        .into_py_any(py)
    }
}

/// A builder for creating custom tags with dynamic names.
///
/// This allows users to create tags like `@karva.tags.some_tag` or `@karva.tags.some_tag(arg1, arg2)`.
#[derive(Debug, Clone)]
#[pyclass(name = "CustomTagBuilder")]
pub struct CustomTagBuilder {
    pub(crate) tag_name: String,
}

#[pymethods]
impl CustomTagBuilder {
    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let custom_tag = PyTag::Custom {
            tag_name: self.tag_name.clone(),
            tag_args: vec![],
            tag_kwargs: vec![],
        };

        // Check if the first argument is a function or test case (decorator without parentheses: @karva.tags.some_tag)
        if args.len() == 1 && kwargs.is_none() {
            if let Ok(first_item) = args.get_item(0) {
                // Check if it's already a PyTestFunction (stacked decorators)
                if let Ok(test_case) = first_item.extract::<PyRef<PyTestFunction>>() {
                    let mut new_tags = test_case.tags.inner.clone();
                    new_tags.push(custom_tag);
                    return PyTestFunction {
                        tags: PyTags { inner: new_tags },
                        function: test_case.function.clone_ref(py),
                    }
                    .into_py_any(py);
                }
                // Check if it's a PyTags object
                if let Ok(tag_obj) = first_item.extract::<PyRef<PyTags>>() {
                    let mut new_tags = tag_obj.inner.clone();
                    new_tags.push(custom_tag);
                    return PyTags { inner: new_tags }.into_py_any(py);
                }
                // Check if it's a plain callable function
                if first_item.is_callable() {
                    return PyTestFunction {
                        tags: PyTags {
                            inner: vec![custom_tag],
                        },
                        function: first_item.unbind(),
                    }
                    .into_py_any(py);
                }
            }
        }

        // Otherwise, create a PyTags that can be used as a decorator: @karva.tags.some_tag(arg1, arg2)
        let args_vec: Vec<Py<PyAny>> = args.iter().map(pyo3::Bound::unbind).collect();
        let kwargs_vec: Vec<(String, Py<PyAny>)> = if let Some(kw) = kwargs {
            kw.iter()
                .filter_map(|(key, value): (Bound<'_, PyAny>, Bound<'_, PyAny>)| {
                    let key_str = key.extract::<String>().ok()?;
                    Some((key_str, value.unbind()))
                })
                .collect()
        } else {
            Vec::new()
        };

        PyTags {
            inner: vec![PyTag::Custom {
                tag_name: self.tag_name.clone(),
                tag_args: args_vec,
                tag_kwargs: kwargs_vec,
            }],
        }
        .into_py_any(py)
    }
}

#[derive(Debug)]
#[pyclass(name = "TestFunction")]
pub struct PyTestFunction {
    #[pyo3(get)]
    pub tags: PyTags,
    pub function: Py<PyAny>,
}

#[pymethods]
impl PyTestFunction {
    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        self.function.call(py, args, kwargs)
    }
}
