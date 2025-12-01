use pyo3::{
    IntoPyObjectExt,
    exceptions::PyTypeError,
    prelude::*,
    types::{PyDict, PyTuple},
};

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
}

#[pymethods]
impl PyTag {
    pub fn name(&self) -> String {
        match self {
            Self::Parametrize { .. } => "parametrize".to_string(),
            Self::UseFixtures { .. } => "use_fixtures".to_string(),
            Self::Skip { .. } => "skip".to_string(),
            Self::ExpectFail { .. } => "expect_fail".to_string(),
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
    use pyo3::{IntoPyObjectExt, exceptions::PyTypeError, prelude::*, types::PyTuple};

    use super::{PyTag, PyTags};
    use crate::extensions::{
        functions::python::Param,
        tags::{parametrize::parse_parametrize_args, python::PyTestFunction},
    };

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

#[cfg(test)]
mod tests {
    use pyo3::{
        ffi::c_str,
        prelude::*,
        types::{PyDict, PyTuple},
    };

    use crate::{
        extensions::tags::python::{PyTag, PyTestFunction, tags},
        utils::attach,
    };

    #[test]
    fn test_parametrize_single_arg() {
        attach(|py| {
            let arg_names = py.eval(c_str!("'a'"), None, None).unwrap();
            let arg_values = py.eval(c_str!("[1, 2, 3]"), None, None).unwrap();
            let tags = tags::parametrize(&arg_names, &arg_values).unwrap();
            assert_eq!(tags.inner.len(), 1);
            assert_eq!(tags.inner[0].name(), "parametrize");
            if let PyTag::Parametrize {
                arg_names,
                arg_values,
            } = &tags.inner[0]
            {
                assert_eq!(arg_names, &vec!["a"]);
                assert_eq!(arg_values.len(), 3);
                assert_eq!(
                    arg_values[0]
                        .values()
                        .first()
                        .unwrap()
                        .extract::<i32>(py)
                        .unwrap(),
                    1
                );
                assert_eq!(
                    arg_values[1]
                        .values()
                        .first()
                        .unwrap()
                        .extract::<i32>(py)
                        .unwrap(),
                    2
                );
                assert_eq!(
                    arg_values[2]
                        .values()
                        .first()
                        .unwrap()
                        .extract::<i32>(py)
                        .unwrap(),
                    3
                );
            }
        });
    }

    #[test]
    fn test_parametrize_multiple_args() {
        attach(|py| {
            let arg_names = py.eval(c_str!("('a', 'b')"), None, None).unwrap();
            let arg_values = py.eval(c_str!("[[1, 2], [3, 4]]"), None, None).unwrap();
            let tags = tags::parametrize(&arg_names, &arg_values).unwrap();
            assert_eq!(tags.inner.len(), 1);
            assert_eq!(tags.inner[0].name(), "parametrize");
            if let PyTag::Parametrize {
                arg_names,
                arg_values,
            } = &tags.inner[0]
            {
                assert_eq!(arg_names, &vec!["a", "b"]);
                assert_eq!(arg_values.len(), 2);
                assert_eq!(arg_values[0].values().len(), 2);
                assert_eq!(arg_values[0].values()[0].extract::<i32>(py).unwrap(), 1);
                assert_eq!(arg_values[0].values()[1].extract::<i32>(py).unwrap(), 2);
                assert_eq!(arg_values[1].values().len(), 2);
                assert_eq!(arg_values[1].values()[0].extract::<i32>(py).unwrap(), 3);
                assert_eq!(arg_values[1].values()[1].extract::<i32>(py).unwrap(), 4);
            }
        });
    }

    #[test]
    fn test_invalid_parametrize_args() {
        attach(|py| {
            let arg_names = py.eval(c_str!("1"), None, None).unwrap();
            let arg_values = py.eval(c_str!("[1, 2, 3]"), None, None).unwrap();
            let tags = tags::parametrize(&arg_names, &arg_values).unwrap_err();
            assert_eq!(
                tags.to_string(),
                "TypeError: Expected a string or a list of strings for the arg_names, and a list of lists of objects for the arg_values"
            );
        });
    }

    #[test]
    fn test_parametrize_multiple_args_with_fixture() {
        attach(|py| {
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
            let test_function = test_function.cast::<PyTestFunction>().unwrap();

            let args = PyTuple::new(py, [1]).unwrap();

            test_function.call1(&args).unwrap();
        });
    }

    #[test]
    fn test_use_fixtures_single_fixture() {
        attach(|py| {
            let binding = py.eval(c_str!("('my_fixture',)"), None, None).unwrap();
            let fixture_names = binding.cast::<PyTuple>().unwrap();
            let tags = tags::use_fixtures(fixture_names).unwrap();
            assert_eq!(tags.inner.len(), 1);
            assert_eq!(tags.inner[0].name(), "use_fixtures");
            if let PyTag::UseFixtures { fixture_names } = &tags.inner[0] {
                assert_eq!(fixture_names, &vec!["my_fixture"]);
            }
        });
    }

    #[test]
    fn test_use_fixtures_multiple_fixtures() {
        attach(|py| {
            let binding = py
                .eval(c_str!("('fixture1', 'fixture2', 'fixture3')"), None, None)
                .unwrap();
            let fixture_names = binding.cast::<PyTuple>().unwrap();
            let tags = tags::use_fixtures(fixture_names).unwrap();
            assert_eq!(tags.inner.len(), 1);
            assert_eq!(tags.inner[0].name(), "use_fixtures");
            if let PyTag::UseFixtures { fixture_names } = &tags.inner[0] {
                assert_eq!(fixture_names, &vec!["fixture1", "fixture2", "fixture3"]);
            }
        });
    }

    #[test]
    fn test_use_fixtures_invalid_args() {
        attach(|py| {
            let locals = PyDict::new(py);

            py.run(
                c_str!(
                    r#"
class BadStr:
    def __str__(self):
        raise Exception("fail str")
bad_tuple = (BadStr(),)
"#
                ),
                None,
                Some(&locals),
            )
            .unwrap();
            let binding = locals.get_item("bad_tuple").unwrap().unwrap();
            let fixture_names = binding.cast::<PyTuple>().unwrap();
            let result = tags::use_fixtures(fixture_names);
            assert!(result.is_err());
            assert_eq!(
                result.unwrap_err().to_string(),
                "TypeError: Expected a string or a list of strings for fixture names"
            );
        });
    }

    #[test]
    fn test_use_fixtures_decorator() {
        attach(|py| {
            let locals = PyDict::new(py);

            py.run(
                c_str!(
                    r#"
import karva

@karva.tags.use_fixtures('fixture1', 'fixture2')
def test_function():
    pass
            "#
                ),
                None,
                Some(&locals),
            )
            .unwrap();

            let test_function = locals.get_item("test_function").unwrap().unwrap();
            let test_function = test_function.cast::<PyTestFunction>().unwrap();

            assert_eq!(test_function.borrow().tags.inner.len(), 1);
            if let PyTag::UseFixtures { fixture_names } = &test_function.borrow().tags.inner[0] {
                assert_eq!(fixture_names, &vec!["fixture1", "fixture2"]);
            }
        });
    }
}
