use std::{ops::Deref, sync::Arc};

use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::extensions::tags::python::{PyTag, PyTags, PyTestFunction};

pub mod custom;
pub mod expect_fail;
pub mod parametrize;
pub mod python;
pub mod skip;
mod use_fixtures;

use custom::CustomTag;
use expect_fail::ExpectFailTag;
use parametrize::{ParametrizationArgs, ParametrizeTag};
use skip::SkipTag;
use use_fixtures::UseFixturesTag;

/// Represents a decorator function in Python that can be used to extend the functionality of a test.
#[derive(Debug, Clone)]
pub enum Tag {
    Parametrize(ParametrizeTag),
    UseFixtures(UseFixturesTag),
    Skip(SkipTag),
    ExpectFail(ExpectFailTag),
    /// Custom tag/marker with arbitrary name, args, and kwargs.
    /// The inner `CustomTag` is stored for future API use but not currently accessed.
    #[expect(dead_code)]
    Custom(CustomTag),
}

impl Tag {
    /// Converts a Pytest mark into an Karva Tag.
    ///
    /// This is used to allow Pytest marks to be used as Karva tags.
    fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let name = py_mark.getattr("name").ok()?.extract::<String>().ok()?;
        match name.as_str() {
            "parametrize" => ParametrizeTag::try_from_pytest_mark(py_mark).map(Self::Parametrize),
            "usefixtures" => UseFixturesTag::try_from_pytest_mark(py_mark).map(Self::UseFixtures),
            "skip" | "skipif" => SkipTag::try_from_pytest_mark(py_mark).map(Self::Skip),
            "xfail" => ExpectFailTag::try_from_pytest_mark(py_mark).map(Self::ExpectFail),
            // Any other marker is treated as a custom marker
            _ => CustomTag::try_from_pytest_mark(py_mark).map(Self::Custom),
        }
    }

    /// Try to create a tag object from a Python object.
    ///
    /// We first check if the object is a `PyTag` or `PyTags`.
    /// If not, we try to call it to see if it returns a `PyTag` or `PyTags`.
    pub(crate) fn try_from_py_any(py: Python, py_any: &Py<PyAny>) -> Option<Self> {
        if let Ok(tag) = py_any.cast_bound::<PyTag>(py) {
            return Some(Self::from_karva_tag(py, tag.borrow()));
        } else if let Ok(tag) = py_any.cast_bound::<PyTags>(py)
            && let Some(tag) = tag.borrow().inner.first()
        {
            return Some(Self::from_karva_tag(py, tag));
        } else if let Ok(tag) = py_any.call0(py) {
            if let Ok(tag) = tag.cast_bound::<PyTag>(py) {
                return Some(Self::from_karva_tag(py, tag.borrow()));
            }
            if let Ok(tag) = tag.cast_bound::<PyTags>(py)
                && let Some(tag) = tag.borrow().inner.first()
            {
                return Some(Self::from_karva_tag(py, tag));
            }
        }

        None
    }

    /// Converts a Karva Python tag into our internal representation.
    pub(crate) fn from_karva_tag<T>(py: Python, py_tag: T) -> Self
    where
        T: Deref<Target = PyTag>,
    {
        match &*py_tag {
            PyTag::Parametrize {
                arg_names,
                arg_values,
            } => Self::Parametrize(ParametrizeTag::from_karva(
                arg_names.clone(),
                arg_values.clone(),
            )),
            PyTag::UseFixtures { fixture_names } => {
                Self::UseFixtures(UseFixturesTag::new(fixture_names.clone()))
            }
            PyTag::Skip { conditions, reason } => {
                Self::Skip(SkipTag::new(conditions.clone(), reason.clone()))
            }
            PyTag::ExpectFail { conditions, reason } => {
                Self::ExpectFail(ExpectFailTag::new(conditions.clone(), reason.clone()))
            }
            PyTag::Custom {
                tag_name,
                tag_args,
                tag_kwargs,
            } => Self::Custom(CustomTag::new(
                tag_name.clone(),
                tag_args.iter().map(|a| Arc::new(a.clone_ref(py))).collect(),
                tag_kwargs
                    .iter()
                    .map(|(k, v)| (k.clone(), Arc::new(v.clone_ref(py))))
                    .collect(),
            )),
        }
    }
}

/// Represents a collection of tags associated with a test function.
///
/// This means we can collect tags and use them all for the same function.
#[derive(Debug, Clone, Default)]
pub struct Tags {
    inner: Vec<Tag>,
}

impl Tags {
    pub(crate) fn new(tags: Vec<Tag>) -> Self {
        Self { inner: tags }
    }

    pub(crate) fn extend(&mut self, other: &Self) {
        self.inner.extend(other.inner.iter().cloned());
    }

    pub(crate) fn from_py_any(
        py: Python<'_>,
        py_function: &Py<PyAny>,
        function_definition: Option<&StmtFunctionDef>,
    ) -> Self {
        if function_definition.is_some_and(|def| def.decorator_list.is_empty()) {
            return Self::default();
        }

        if let Ok(py_test_function) = py_function.extract::<Py<PyTestFunction>>(py) {
            let mut tags = Vec::new();
            for tag in &py_test_function.borrow(py).tags.inner {
                tags.push(Tag::from_karva_tag(py, tag));
            }
            return Self::new(tags);
        } else if let Ok(wrapped) = py_function.getattr(py, "__wrapped__") {
            if let Ok(py_wrapped_function) = wrapped.extract::<Py<PyTestFunction>>(py) {
                let mut tags = Vec::new();
                for tag in &py_wrapped_function.borrow(py).tags.inner {
                    tags.push(Tag::from_karva_tag(py, tag));
                }
                return Self::new(tags);
            }
        }

        if let Ok(marks) = py_function.getattr(py, "pytestmark")
            && let Some(tags) = Self::from_pytest_marks(py, &marks)
        {
            return tags;
        }

        Self::default()
    }

    pub(crate) fn from_pytest_marks(py: Python<'_>, marks: &Py<PyAny>) -> Option<Self> {
        let mut tags = Vec::new();
        if let Ok(marks_list) = marks.extract::<Vec<Bound<'_, PyAny>>>(py) {
            for mark in marks_list {
                if let Some(tag) = Tag::try_from_pytest_mark(&mark) {
                    tags.push(tag);
                }
            }
        } else {
            return None;
        }
        Some(Self { inner: tags })
    }

    /// Return all parametrizations
    ///
    /// This function ensures that if we have multiple parametrize tags, we combine them together.
    pub(crate) fn parametrize_args(&self) -> Vec<ParametrizationArgs> {
        let mut param_args: Vec<ParametrizationArgs> = vec![ParametrizationArgs::default()];

        for tag in &self.inner {
            if let Tag::Parametrize(parametrize_tag) = tag {
                let current_values = parametrize_tag.each_arg_value();

                let mut new_param_args =
                    Vec::with_capacity(param_args.len() * current_values.len());

                for existing_params in &param_args {
                    for new_params in &current_values {
                        let mut combined_params = existing_params.clone();
                        combined_params.extend(new_params.clone());
                        new_param_args.push(combined_params);
                    }
                }
                param_args = new_param_args;
            }
        }
        param_args
    }

    /// Get all required fixture names for the given test.
    pub(crate) fn required_fixtures_names(&self) -> Vec<String> {
        let mut fixture_names = Vec::new();
        for tag in &self.inner {
            if let Tag::UseFixtures(use_fixtures_tag) = tag {
                fixture_names.extend_from_slice(use_fixtures_tag.fixture_names());
            }
        }
        fixture_names
    }

    /// Returns true if any skip tag should be skipped.
    pub(crate) fn should_skip(&self) -> (bool, Option<String>) {
        for tag in &self.inner {
            if let Tag::Skip(skip_tag) = tag {
                if skip_tag.should_skip() {
                    return (true, skip_tag.reason());
                }
            }
        }
        (false, None)
    }

    /// Return the `ExpectFailTag` if it exists.
    pub(crate) fn expect_fail_tag(&self) -> Option<ExpectFailTag> {
        for tag in &self.inner {
            if let Tag::ExpectFail(expect_fail_tag) = tag {
                return Some(expect_fail_tag.clone());
            }
        }
        None
    }
}
