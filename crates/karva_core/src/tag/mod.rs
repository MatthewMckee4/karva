use std::collections::HashMap;

use pyo3::prelude::*;

use crate::tag::python::{PyTag, PyTestFunction};

pub mod python;

#[derive(Debug, Clone)]
pub enum Tag {
    Parametrize(ParametrizeTag),
}

impl Tag {
    #[must_use]
    pub fn from_py_tag(py_tag: &PyTag) -> Self {
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
    pub fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        py_mark.getattr("name").map_or_else(
            |_| None,
            |name| {
                name.extract::<String>().map_or_else(
                    |_| None,
                    |name_str| {
                        if name_str == "parametrize" {
                            ParametrizeTag::try_from_pytest_mark(py_mark).map(Self::Parametrize)
                        } else {
                            None
                        }
                    },
                )
            },
        )
    }

    pub fn try_from_karva_tag(py_tag: &Bound<'_, PyAny>) -> PyResult<Option<Self>> {
        let name_str = py_tag.call_method0("name")?.extract::<String>()?;

        if name_str == "parametrize" {
            Ok(ParametrizeTag::try_from_karva_tag(py_tag)?.map(Self::Parametrize))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug, Clone)]
pub struct ParametrizeTag {
    pub arg_names: Vec<String>,
    pub arg_values: Vec<Vec<PyObject>>,
}

impl ParametrizeTag {
    #[must_use]
    pub fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
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

    pub fn try_from_karva_tag(py_tag: &Bound<'_, PyAny>) -> PyResult<Option<Self>> {
        let arg_names = py_tag.getattr("arg_names")?;
        let arg_values = py_tag.getattr("arg_values")?;

        if let (Ok(name), Ok(values)) = (
            arg_names.extract::<String>(),
            arg_values.extract::<Vec<PyObject>>(),
        ) {
            Ok(Some(Self {
                arg_names: vec![name],
                arg_values: values.into_iter().map(|v| vec![v]).collect(),
            }))
        } else if let (Ok(names), Ok(values)) = (
            arg_names.extract::<Vec<String>>(),
            arg_values.extract::<Vec<Vec<PyObject>>>(),
        ) {
            Ok(Some(Self {
                arg_names: names,
                arg_values: values,
            }))
        } else {
            Ok(None)
        }
    }

    #[must_use]
    pub fn each_arg_value(&self) -> Vec<HashMap<String, PyObject>> {
        let mut param_args: Vec<HashMap<String, PyObject>> = Vec::new();
        for values in &self.arg_values {
            let mut current_parameratisation = HashMap::new();
            for (arg_name, arg_value) in self.arg_names.iter().zip(values.iter()) {
                current_parameratisation.insert(arg_name.clone(), arg_value.clone());
            }
            param_args.push(current_parameratisation);
        }
        param_args
    }
}

#[derive(Debug, Clone, Default)]
pub struct Tags {
    pub inner: Vec<Tag>,
}

impl Tags {
    #[must_use]
    pub const fn new(inner: Vec<Tag>) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn from_py_any(py: Python<'_>, py_test_function: &Py<PyAny>) -> Self {
        if let Ok(py_test_function) = py_test_function.extract::<Py<PyTestFunction>>(py) {
            let mut tags = Vec::new();
            for tag in &py_test_function.borrow(py).tags.inner {
                tags.push(Tag::from_py_tag(tag));
            }
            return Self { inner: tags };
        }

        if let Some(tags) = Self::from_pytest_function(py, py_test_function) {
            return tags;
        }

        let tags = Self::try_from_unresolved_karva_tags(py, py_test_function);

        if let Ok(tags) = tags {
            return tags;
        }

        Self::default()
    }

    #[must_use]
    pub fn from_pytest_function(py: Python<'_>, py_test_function: &Py<PyAny>) -> Option<Self> {
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

    pub fn try_from_unresolved_karva_tags(
        py: Python<'_>,
        py_test_function: &Py<PyAny>,
    ) -> PyResult<Self> {
        let tags_attr = py_test_function.getattr(py, "tags")?;
        let tags_iter = tags_attr.call_method0(py, "__iter__")?;
        let tags_list = tags_iter.extract::<Vec<Bound<'_, PyAny>>>(py)?;

        let mut tags = Vec::new();
        for tag in tags_list {
            if let Ok(Some(parsed_tag)) = Tag::try_from_karva_tag(&tag) {
                tags.push(parsed_tag);
            }
        }

        Ok(Self { inner: tags })
    }
}

impl Iterator for Tags {
    type Item = Tag;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.pop()
    }
}
