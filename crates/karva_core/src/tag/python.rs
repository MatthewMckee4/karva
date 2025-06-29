use pyo3::{
    IntoPyObjectExt,
    exceptions::PyTypeError,
    prelude::*,
    types::{PyDict, PyFunction, PyTuple, PyType},
};

#[derive(Debug, Clone)]
#[pyclass(name = "tag")]
pub enum PyTag {
    #[pyo3(name = "parametrize")]
    Parametrize {
        arg_names: Vec<String>,
        arg_values: Vec<Vec<PyObject>>,
    },
}

#[pymethods]
impl PyTag {
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            Self::Parametrize { .. } => "parametrize".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
#[pyclass(name = "tags")]
pub struct PyTags {
    pub inner: Vec<PyTag>,
}

#[pymethods]
impl PyTags {
    #[classmethod]
    pub fn parametrize(
        _cls: &Bound<'_, PyType>,
        arg_names: &Bound<'_, PyAny>,
        arg_values: &Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        if let (Ok(name), Ok(values)) = (
            arg_names.extract::<String>(),
            arg_values.extract::<Vec<PyObject>>(),
        ) {
            Ok(Self {
                inner: vec![PyTag::Parametrize {
                    arg_names: vec![name],
                    arg_values: values.into_iter().map(|v| vec![v]).collect(),
                }],
            })
        } else if let (Ok(names), Ok(values)) = (
            arg_names.extract::<Vec<String>>(),
            arg_values.extract::<Vec<Vec<PyObject>>>(),
        ) {
            Ok(Self {
                inner: vec![PyTag::Parametrize {
                    arg_names: names,
                    arg_values: values,
                }],
            })
        } else {
            Err(PyErr::new::<PyTypeError, _>(
                "Expected a string or a list of strings for the arg_names, and a list of lists of objects for the arg_values",
            ))
        }
    }

    #[pyo3(signature = (f, /))]
    pub fn __call__(&self, py: Python<'_>, f: PyObject) -> PyResult<PyObject> {
        if let Ok(tag_obj) = f.downcast_bound::<Self>(py) {
            tag_obj.borrow_mut().inner.extend(self.inner.clone());
            return tag_obj.into_py_any(py);
        } else if let Ok(test_case) = f.downcast_bound::<PyTestFunction>(py) {
            test_case.borrow_mut().tags.inner.extend(self.inner.clone());
            return test_case.into_py_any(py);
        } else if f.extract::<Py<PyFunction>>(py).is_ok() {
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

    pub fn __iter__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.inner.clone().into_py_any(py)
    }
}

#[derive(Debug)]
#[pyclass(name = "TestFunction")]
pub struct PyTestFunction {
    #[pyo3(get)]
    pub tags: PyTags,
    pub function: PyObject,
}

#[pymethods]
impl PyTestFunction {
    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        self.function.call(py, args, kwargs)
    }
}

#[cfg(test)]
mod tests {
    use pyo3::{
        ffi::c_str,
        prelude::*,
        types::{PyDict, PyTuple, PyType},
    };

    use crate::tag::python::{PyTag, PyTags, PyTestFunction};

    #[test]
    fn test_parametrize_single_arg() {
        Python::with_gil(|py| {
            let locals = PyDict::new(py);
            Python::run(
                py,
                c_str!("import karva;tags = karva.tags"),
                None,
                Some(&locals),
            )
            .unwrap();

            let binding = locals.get_item("tags").unwrap().unwrap();
            let cls = binding.downcast::<PyType>().unwrap();

            let arg_names = py.eval(c_str!("'a'"), None, None).unwrap();
            let arg_values = py.eval(c_str!("[1, 2, 3]"), None, None).unwrap();
            let tags = PyTags::parametrize(cls, &arg_names, &arg_values).unwrap();
            assert_eq!(tags.inner.len(), 1);
            assert_eq!(tags.inner[0].name(), "parametrize");
            let PyTag::Parametrize {
                arg_names,
                arg_values,
            } = &tags.inner[0];
            assert_eq!(arg_names, &vec!["a"]);
            assert_eq!(arg_values.len(), 3);
            assert_eq!(
                arg_values[0].first().unwrap().extract::<i32>(py).unwrap(),
                1
            );
            assert_eq!(
                arg_values[1].first().unwrap().extract::<i32>(py).unwrap(),
                2
            );
            assert_eq!(
                arg_values[2].first().unwrap().extract::<i32>(py).unwrap(),
                3
            );
        });
    }

    #[test]
    fn test_parametrize_multiple_args() {
        Python::with_gil(|py| {
            let locals = PyDict::new(py);
            Python::run(
                py,
                c_str!("import karva;tags = karva.tags"),
                None,
                Some(&locals),
            )
            .unwrap();

            let binding = locals.get_item("tags").unwrap().unwrap();
            let cls = binding.downcast::<PyType>().unwrap();

            let arg_names = py.eval(c_str!("('a', 'b')"), None, None).unwrap();
            let arg_values = py.eval(c_str!("[[1, 2], [3, 4]]"), None, None).unwrap();
            let tags = PyTags::parametrize(cls, &arg_names, &arg_values).unwrap();
            assert_eq!(tags.inner.len(), 1);
            assert_eq!(tags.inner[0].name(), "parametrize");
            let PyTag::Parametrize {
                arg_names,
                arg_values,
            } = &tags.inner[0];
            assert_eq!(arg_names, &vec!["a", "b"]);
            assert_eq!(arg_values.len(), 2);
            assert_eq!(arg_values[0].len(), 2);
            assert_eq!(arg_values[0][0].extract::<i32>(py).unwrap(), 1);
            assert_eq!(arg_values[0][1].extract::<i32>(py).unwrap(), 2);
            assert_eq!(arg_values[1].len(), 2);
            assert_eq!(arg_values[1][0].extract::<i32>(py).unwrap(), 3);
            assert_eq!(arg_values[1][1].extract::<i32>(py).unwrap(), 4);
        });
    }

    #[test]
    fn test_invalid_parametrize_args() {
        Python::with_gil(|py| {
            let locals = PyDict::new(py);
            Python::run(
                py,
                c_str!("import karva;tags = karva.tags"),
                None,
                Some(&locals),
            )
            .unwrap();

            let binding = locals.get_item("tags").unwrap().unwrap();
            let cls = binding.downcast::<PyType>().unwrap();

            let arg_names = py.eval(c_str!("1"), None, None).unwrap();
            let arg_values = py.eval(c_str!("[1, 2, 3]"), None, None).unwrap();
            let tags = PyTags::parametrize(cls, &arg_names, &arg_values).unwrap_err();
            assert_eq!(
                tags.to_string(),
                "TypeError: Expected a string or a list of strings for the arg_names, and a list of lists of objects for the arg_values"
            );
        });
    }

    #[test]
    fn test_parametrize_multiple_args_with_fixture() {
        Python::with_gil(|py| {
            let locals = PyDict::new(py);

            py.run(
                c_str!(
                    r#"
import karva

@karva.tags.parametrize("a", [1, 2, 3])
def test_function(a):
    assert a > 0
            "#
                ),
                None,
                Some(&locals),
            )
            .unwrap();

            let test_function = locals.get_item("test_function").unwrap().unwrap();
            let test_function = test_function.downcast::<PyTestFunction>().unwrap();

            let args = PyTuple::new(py, [1]).unwrap();

            test_function.call1(&args).unwrap();
        });
    }
}
