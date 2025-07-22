use std::collections::HashMap;

use pyo3::prelude::*;

use crate::extensions::tags::python::{PyTag, PyTestFunction};

pub mod python;

#[derive(Debug, Clone)]
pub(crate) enum Tag {
    Parametrize(ParametrizeTag),
}

impl Tag {
    #[must_use]
    pub(crate) fn from_py_tag(py_tag: &PyTag) -> Self {
        match py_tag {
            PyTag::Parametrize {
                arg_names,
                arg_values,
            } => Self::Parametrize(ParametrizeTag {
                arg_names: arg_names.clone(),
                arg_values: arg_values.clone(),
            }),
        }
    }

    #[must_use]
    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let name = py_mark.getattr("name").ok()?.extract::<String>().ok()?;
        if name == "parametrize" {
            ParametrizeTag::try_from_pytest_mark(py_mark).map(Self::Parametrize)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ParametrizeTag {
    pub(crate) arg_names: Vec<String>,
    pub(crate) arg_values: Vec<Vec<PyObject>>,
}

impl ParametrizeTag {
    #[must_use]
    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let args = py_mark.getattr("args").ok()?;
        if let Ok((arg_name, arg_values)) = args.extract::<(String, Vec<PyObject>)>() {
            Some(Self {
                arg_names: vec![arg_name],
                arg_values: arg_values.into_iter().map(|v| vec![v]).collect(),
            })
        } else if let Ok((arg_names, arg_values)) =
            args.extract::<(Vec<String>, Vec<Vec<PyObject>>)>()
        {
            Some(Self {
                arg_names,
                arg_values,
            })
        } else {
            None
        }
    }

    #[must_use]
    pub(crate) fn each_arg_value(&self) -> Vec<HashMap<String, PyObject>> {
        let total_combinations = self.arg_values.len();
        let mut param_args = Vec::with_capacity(total_combinations);

        for values in &self.arg_values {
            let mut current_parameratisation = HashMap::with_capacity(self.arg_names.len());
            for (arg_name, arg_value) in self.arg_names.iter().zip(values.iter()) {
                current_parameratisation.insert(arg_name.clone(), arg_value.clone());
            }
            param_args.push(current_parameratisation);
        }
        param_args
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Tags {
    inner: Vec<Tag>,
}

impl Tags {
    #[must_use]
    pub(crate) fn from_py_any(py: Python<'_>, py_test_function: &Py<PyAny>) -> Self {
        if let Ok(py_test_function) = py_test_function.extract::<Py<PyTestFunction>>(py) {
            let mut tags = Vec::new();
            for tag in &py_test_function.borrow(py).tags.inner {
                tags.push(Tag::from_py_tag(tag));
            }
            return Self { inner: tags };
        } else if let Ok(wrapped) = py_test_function.getattr(py, "__wrapped__") {
            if let Ok(py_wrapped_function) = wrapped.extract::<Py<PyTestFunction>>(py) {
                let mut tags = Vec::new();
                for tag in &py_wrapped_function.borrow(py).tags.inner {
                    tags.push(Tag::from_py_tag(tag));
                }
                return Self { inner: tags };
            }
        }

        if let Some(tags) = Self::from_pytest_function(py, py_test_function) {
            return tags;
        }

        Self::default()
    }

    #[must_use]
    pub(crate) fn from_pytest_function(
        py: Python<'_>,
        py_test_function: &Py<PyAny>,
    ) -> Option<Self> {
        let mut tags = Vec::new();
        if let Ok(marks) = py_test_function.getattr(py, "pytestmark") {
            if let Ok(marks_list) = marks.extract::<Vec<Bound<'_, PyAny>>>(py) {
                for mark in marks_list {
                    if let Some(tag) = Tag::try_from_pytest_mark(&mark) {
                        tags.push(tag);
                    }
                }
            }
        } else {
            return None;
        }
        Some(Self { inner: tags })
    }

    #[must_use]
    pub(crate) fn parametrize_args(&self) -> Vec<HashMap<String, PyObject>> {
        let mut param_args: Vec<HashMap<String, PyObject>> = vec![HashMap::new()];

        for tag in &self.inner {
            let Tag::Parametrize(parametrize_tag) = tag;
            let current_values = parametrize_tag.each_arg_value();
            let mut new_param_args = Vec::with_capacity(param_args.len() * current_values.len());
            for existing_params in &param_args {
                for new_params in &current_values {
                    let mut combined_params = existing_params.clone();
                    combined_params.extend(new_params.clone());
                    new_param_args.push(combined_params);
                }
            }
            param_args = new_param_args;
        }
        param_args
    }
}

impl Iterator for Tags {
    type Item = Tag;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.pop()
    }
}

#[cfg(test)]
mod tests {
    use pyo3::{ffi::c_str, prelude::*, types::PyDict};

    use super::*;

    #[test]
    fn test_parametrize_args_single_arg() {
        Python::with_gil(|py| {
            let locals = PyDict::new(py);
            Python::run(
                py,
                c_str!(
                    r#"
import karva

@karva.tags.parametrize("a", [1, 2, 3])
def test_parametrize(a):
    pass
                "#
                ),
                None,
                Some(&locals),
            )
            .unwrap();

            let test_function = locals.get_item("test_parametrize").unwrap().unwrap();

            let test_function = test_function.as_unbound();

            let tags = Tags::from_py_any(py, test_function);

            let expected_parametrize_args = [
                HashMap::from([(String::from("a"), 1)]),
                HashMap::from([(String::from("a"), 2)]),
                HashMap::from([(String::from("a"), 3)]),
            ];

            for (i, parametrize_arg) in tags.parametrize_args().iter().enumerate() {
                for (key, value) in parametrize_arg {
                    assert_eq!(
                        value.extract::<i32>(py).unwrap(),
                        expected_parametrize_args[i][&key.to_string()]
                    );
                }
            }
        });
    }

    #[test]
    fn test_parametrize_args_two_args() {
        Python::with_gil(|py| {
            let locals = PyDict::new(py);
            Python::run(
                py,
                c_str!(
                    r#"
import karva

@karva.tags.parametrize(("a", "b"), [(1, 4), (2, 5), (3, 6)])
def test_parametrize(a, b):
    pass
                "#
                ),
                None,
                Some(&locals),
            )
            .unwrap();

            let test_function = locals.get_item("test_parametrize").unwrap().unwrap();

            let test_function = test_function.as_unbound();

            let tags = Tags::from_py_any(py, test_function);

            let expected_parametrize_args = [
                HashMap::from([(String::from("a"), 1), (String::from("b"), 4)]),
                HashMap::from([(String::from("a"), 2), (String::from("b"), 5)]),
                HashMap::from([(String::from("a"), 3), (String::from("b"), 6)]),
            ];

            for (i, parametrize_arg) in tags.parametrize_args().iter().enumerate() {
                for (key, value) in parametrize_arg {
                    assert_eq!(
                        value.extract::<i32>(py).unwrap(),
                        expected_parametrize_args[i][&key.to_string()]
                    );
                }
            }
        });
    }

    #[test]
    fn test_parametrize_args_multiple_tags() {
        Python::with_gil(|py| {
            let locals = PyDict::new(py);
            Python::run(
                py,
                c_str!(
                    r#"
import karva

@karva.tags.parametrize("a", [1, 2, 3])
@karva.tags.parametrize("b", [4, 5, 6])
def test_parametrize(a):
    pass
                "#
                ),
                None,
                Some(&locals),
            )
            .unwrap();

            let test_function = locals.get_item("test_parametrize").unwrap().unwrap();

            let test_function = test_function.as_unbound();

            let tags = Tags::from_py_any(py, test_function);

            let expected_parametrize_args = [
                HashMap::from([(String::from("a"), 1), (String::from("b"), 4)]),
                HashMap::from([(String::from("a"), 2), (String::from("b"), 4)]),
                HashMap::from([(String::from("a"), 3), (String::from("b"), 4)]),
                HashMap::from([(String::from("a"), 1), (String::from("b"), 5)]),
                HashMap::from([(String::from("a"), 2), (String::from("b"), 5)]),
                HashMap::from([(String::from("a"), 3), (String::from("b"), 5)]),
                HashMap::from([(String::from("a"), 1), (String::from("b"), 6)]),
                HashMap::from([(String::from("a"), 2), (String::from("b"), 6)]),
                HashMap::from([(String::from("a"), 3), (String::from("b"), 6)]),
            ];

            for (i, parametrize_arg) in tags.parametrize_args().iter().enumerate() {
                for (key, value) in parametrize_arg {
                    assert_eq!(
                        value.extract::<i32>(py).unwrap(),
                        expected_parametrize_args[i][&key.to_string()]
                    );
                }
            }
        });
    }
}
