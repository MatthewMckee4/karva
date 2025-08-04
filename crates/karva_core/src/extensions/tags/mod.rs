use std::collections::HashMap;

use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::extensions::tags::python::{PyTag, PyTestFunction};

pub mod python;

#[derive(Debug, Clone)]
pub(crate) enum Tag {
    Parametrize(ParametrizeTag),
    UseFixtures(UseFixturesTag),
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
            PyTag::UseFixtures { fixture_names } => Self::UseFixtures(UseFixturesTag {
                fixture_names: fixture_names.clone(),
            }),
        }
    }

    #[must_use]
    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let name = py_mark.getattr("name").ok()?.extract::<String>().ok()?;
        if name == "parametrize" {
            ParametrizeTag::try_from_pytest_mark(py_mark).map(Self::Parametrize)
        } else if name == "usefixtures" {
            UseFixturesTag::try_from_pytest_mark(py_mark).map(Self::UseFixtures)
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

#[derive(Debug, Clone)]
pub(crate) struct UseFixturesTag {
    pub(crate) fixture_names: Vec<String>,
}

impl UseFixturesTag {
    #[must_use]
    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let args = py_mark.getattr("args").ok()?;
        args.extract::<Vec<String>>().map_or_else(
            |_| {
                args.extract::<String>().map_or(None, |fixture_name| {
                    Some(Self {
                        fixture_names: vec![fixture_name],
                    })
                })
            },
            |fixture_names| Some(Self { fixture_names }),
        )
    }

    #[must_use]
    pub(crate) fn fixture_names(&self) -> &[String] {
        &self.fixture_names
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Tags {
    inner: Vec<Tag>,
}

impl Tags {
    #[must_use]
    pub(crate) fn from_py_any(
        py: Python<'_>,
        py_function: &Py<PyAny>,
        function_definition: Option<&StmtFunctionDef>,
    ) -> Option<Self> {
        if function_definition.is_some_and(|def| def.decorator_list.is_empty()) {
            return None;
        }

        if let Ok(py_test_function) = py_function.extract::<Py<PyTestFunction>>(py) {
            let mut tags = Vec::new();
            for tag in &py_test_function.borrow(py).tags.inner {
                tags.push(Tag::from_py_tag(tag));
            }
            return Some(Self { inner: tags });
        } else if let Ok(wrapped) = py_function.getattr(py, "__wrapped__") {
            if let Ok(py_wrapped_function) = wrapped.extract::<Py<PyTestFunction>>(py) {
                let mut tags = Vec::new();
                for tag in &py_wrapped_function.borrow(py).tags.inner {
                    tags.push(Tag::from_py_tag(tag));
                }
                return Some(Self { inner: tags });
            }
        }

        if let Some(tags) = Self::from_pytest_function(py, py_function) {
            return Some(tags);
        }

        None
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
            if let Tag::Parametrize(parametrize_tag) = tag {
                let current_values = parametrize_tag.each_arg_value();
                let mut new_param_args =
                    Vec::with_capacity(param_args.len() * current_values.len());
                for existing_params in &param_args {
                    for new_params in &current_values {
                        let mut combined_params =
                            HashMap::with_capacity(existing_params.len() + new_params.len());
                        combined_params
                            .extend(existing_params.iter().map(|(k, v)| (k.clone(), v.clone())));
                        combined_params
                            .extend(new_params.iter().map(|(k, v)| (k.clone(), v.clone())));
                        new_param_args.push(combined_params);
                    }
                }
                param_args = new_param_args;
            }
        }
        param_args
    }

    #[must_use]
    pub(crate) fn use_fixtures_names(&self) -> Vec<String> {
        let mut fixture_names = Vec::new();
        for tag in &self.inner {
            if let Tag::UseFixtures(use_fixtures_tag) = tag {
                fixture_names.extend_from_slice(use_fixtures_tag.fixture_names());
            }
        }
        fixture_names
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
    use std::collections::HashSet;

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

            let tags = Tags::from_py_any(py, test_function, None).unwrap();

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

            let tags = Tags::from_py_any(py, test_function, None).unwrap();

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

            let tags = Tags::from_py_any(py, test_function, None).unwrap();

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

    #[test]
    fn test_use_fixtures_names_single() {
        Python::with_gil(|py| {
            let locals = PyDict::new(py);
            Python::run(
                py,
                c_str!(
                    r#"
import karva

@karva.tags.use_fixtures("my_fixture")
def test_function():
    pass
                "#
                ),
                None,
                Some(&locals),
            )
            .unwrap();

            let test_function = locals.get_item("test_function").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None).unwrap();

            let fixture_names = tags.use_fixtures_names();
            assert_eq!(fixture_names, vec!["my_fixture"]);
        });
    }

    #[test]
    fn test_use_fixtures_names_multiple() {
        Python::with_gil(|py| {
            let locals = PyDict::new(py);
            Python::run(
                py,
                c_str!(
                    r#"
import karva

@karva.tags.use_fixtures("fixture1", "fixture2", "fixture3")
def test_function():
    pass
                "#
                ),
                None,
                Some(&locals),
            )
            .unwrap();

            let test_function = locals.get_item("test_function").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None).unwrap();

            let fixture_names = tags.use_fixtures_names();
            assert_eq!(fixture_names, vec!["fixture1", "fixture2", "fixture3"]);
        });
    }

    #[test]
    fn test_use_fixtures_names_multiple_tags() {
        Python::with_gil(|py| {
            let locals = PyDict::new(py);
            Python::run(
                py,
                c_str!(
                    r#"
import karva

@karva.tags.use_fixtures("fixture1", "fixture2")
@karva.tags.use_fixtures("fixture3")
def test_function():
    pass
                "#
                ),
                None,
                Some(&locals),
            )
            .unwrap();

            let test_function = locals.get_item("test_function").unwrap().unwrap();
            let test_function = test_function.as_unbound();
            let tags = Tags::from_py_any(py, test_function, None).unwrap();

            let fixture_names: HashSet<_> = tags.use_fixtures_names().into_iter().collect();
            let expected: HashSet<_> = ["fixture1", "fixture2", "fixture3"]
                .iter()
                .copied()
                .map(String::from)
                .collect();
            assert_eq!(fixture_names, expected);
        });
    }

    #[test]
    fn test_pytest_usefixtures_tag() {
        Python::with_gil(|py| {
            let mark = py.eval(
                c_str!(r#"type('Mark', (), {'name': 'usefixtures', 'args': ['fixture1', 'fixture2']})()"#),
                None,
                None
            ).unwrap();

            let tag = Tag::try_from_pytest_mark(&mark);
            assert!(tag.is_some());
            if let Tag::UseFixtures(use_fixtures_tag) = tag.unwrap() {
                assert_eq!(use_fixtures_tag.fixture_names(), &["fixture1", "fixture2"]);
            }
        });
    }

    #[test]
    fn test_pytest_usefixtures_single_fixture() {
        Python::with_gil(|py| {
            let mark = py
                .eval(
                    c_str!(
                        r#"type('Mark', (), {'name': 'usefixtures', 'args': 'single_fixture'})()"#
                    ),
                    None,
                    None,
                )
                .unwrap();

            let tag = Tag::try_from_pytest_mark(&mark);
            assert!(tag.is_some());
            if let Tag::UseFixtures(use_fixtures_tag) = tag.unwrap() {
                assert_eq!(use_fixtures_tag.fixture_names(), &["single_fixture"]);
            }
        });
    }
}
