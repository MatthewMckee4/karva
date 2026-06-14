use std::{ops::Deref, sync::Arc};

use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::extensions::tags::python::{PyTag, PyTags, PyTestFunction};

pub mod custom;
pub mod expect_fail;
pub mod parametrize;
pub mod python;
pub mod skip;
pub mod timeout;
mod use_fixtures;

use custom::CustomTag;
use expect_fail::ExpectFailTag;
use parametrize::{ParametrizationArgs, ParametrizeTag};
use skip::SkipTag;
use timeout::TimeoutTag;
use use_fixtures::UseFixturesTag;

/// Parsed conditions and reason extracted from a pytest mark's args and kwargs.
///
/// Used by both `SkipTag` and `ExpectFailTag` which share identical parsing logic.
pub struct ParsedMarkArgs {
    pub conditions: Vec<bool>,
    pub reason: Option<String>,
}

/// Extract conditions and reason from a pytest mark object.
///
/// Pytest marks store boolean conditions as positional args and an optional
/// `reason` as a keyword argument. A string in the first positional arg
/// (when no booleans were found) is treated as an old-style positional reason.
pub fn parse_pytest_mark_args(py_mark: &Bound<'_, PyAny>) -> Option<ParsedMarkArgs> {
    let kwargs = py_mark.getattr("kwargs").ok()?;
    let args = py_mark.getattr("args").ok()?;

    let mut conditions = Vec::new();
    if let Ok(args_tuple) = args.extract::<Bound<'_, pyo3::types::PyTuple>>() {
        for i in 0..args_tuple.len() {
            if let Ok(item) = args_tuple.get_item(i) {
                if let Ok(bool_val) = item.extract::<bool>() {
                    conditions.push(bool_val);
                } else if item.extract::<String>().is_ok() {
                    break;
                }
            }
        }
    }

    let reason = if let Ok(reason_item) = kwargs.get_item("reason") {
        reason_item.extract::<String>().ok()
    } else if conditions.is_empty() {
        // Fall back to first positional arg as reason
        args.extract::<Bound<'_, pyo3::types::PyTuple>>()
            .ok()
            .and_then(|t| t.get_item(0).ok())
            .and_then(|a| a.extract::<String>().ok())
    } else {
        None
    };

    Some(ParsedMarkArgs { conditions, reason })
}

/// Represents a decorator/marker that modifies test behavior.
///
/// Tags are extracted from Python decorators like `@pytest.mark.parametrize`,
/// `@pytest.mark.skip`, etc., and control how tests are executed.
#[derive(Debug, Clone)]
pub enum Tag {
    Parametrize(ParametrizeTag),
    UseFixtures(UseFixturesTag),
    Skip(SkipTag),
    ExpectFail(ExpectFailTag),
    Timeout(TimeoutTag),
    Custom(CustomTag),
}

impl Tag {
    /// Converts a Pytest mark into an Karva Tag.
    ///
    /// This is used to allow Pytest marks to be used as Karva tags.
    fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> PyResult<Option<Self>> {
        let Some(name) = py_mark
            .getattr("name")
            .ok()
            .and_then(|name| name.extract::<String>().ok())
        else {
            return Ok(None);
        };

        match name.as_str() {
            "parametrize" => {
                ParametrizeTag::try_from_pytest_mark(py_mark).map(|tag| tag.map(Self::Parametrize))
            }
            "usefixtures" => {
                UseFixturesTag::try_from_pytest_mark(py_mark).map(|tag| tag.map(Self::UseFixtures))
            }
            "skip" | "skipif" => Ok(SkipTag::try_from_pytest_mark(py_mark).map(Self::Skip)),
            "xfail" => Ok(ExpectFailTag::try_from_pytest_mark(py_mark).map(Self::ExpectFail)),
            "timeout" => {
                TimeoutTag::try_from_pytest_mark(py_mark).map(|tag| tag.map(Self::Timeout))
            }
            // Any other marker is treated as a custom marker
            _ => Ok(CustomTag::try_from_pytest_mark(py_mark).map(Self::Custom)),
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
            PyTag::Timeout { seconds } => Self::Timeout(TimeoutTag::new(*seconds)),
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

/// A collection of tags associated with a test function.
///
/// Holds all decorator tags applied to a test, allowing multiple
/// markers (parametrize, skip, xfail, etc.) to be combined.
#[derive(Debug, Clone, Default)]
pub struct Tags {
    /// The list of tags applied to a test function.
    inner: Vec<Tag>,
}

impl Tags {
    pub(crate) fn new(tags: Vec<Tag>) -> Self {
        Self { inner: tags }
    }

    fn from_py_test_function(py: Python<'_>, test_function: &PyTestFunction) -> Self {
        let tags = test_function
            .tags
            .inner
            .iter()
            .map(|tag| Tag::from_karva_tag(py, tag))
            .collect();
        Self::new(tags)
    }

    pub(crate) fn extend(&mut self, other: &Self) {
        self.inner.extend(other.inner.iter().cloned());
    }

    pub(crate) fn from_py_any(
        py: Python<'_>,
        py_function: &Py<PyAny>,
        function_definition: Option<&StmtFunctionDef>,
    ) -> PyResult<Self> {
        if function_definition.is_some_and(|def| def.decorator_list.is_empty()) {
            return Ok(Self::default());
        }

        if let Ok(py_test_function) = py_function.extract::<Py<PyTestFunction>>(py) {
            return Ok(Self::from_py_test_function(
                py,
                &py_test_function.borrow(py),
            ));
        } else if let Ok(wrapped) = py_function.getattr(py, "__wrapped__")
            && let Ok(py_wrapped_function) = wrapped.extract::<Py<PyTestFunction>>(py)
        {
            return Ok(Self::from_py_test_function(
                py,
                &py_wrapped_function.borrow(py),
            ));
        }

        if let Ok(marks) = py_function.getattr(py, "pytestmark")
            && let Some(tags) = Self::from_pytest_marks(py, &marks)?
        {
            return Ok(tags);
        }

        Ok(Self::default())
    }

    pub(crate) fn from_pytest_marks(py: Python<'_>, marks: &Py<PyAny>) -> PyResult<Option<Self>> {
        let mut tags = Vec::new();
        if let Ok(marks_list) = marks.extract::<Vec<Bound<'_, PyAny>>>(py) {
            for mark in marks_list {
                if let Some(tag) = Tag::try_from_pytest_mark(&mark)? {
                    tags.push(tag);
                }
            }
        } else {
            return Ok(None);
        }
        Ok(Some(Self { inner: tags }))
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
        self.inner
            .iter()
            .filter_map(|tag| match tag {
                Tag::UseFixtures(use_fixtures_tag) => Some(use_fixtures_tag.fixture_names()),
                _ => None,
            })
            .flat_map(|names| names.iter().cloned())
            .collect()
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

    /// Return the names of all custom tags.
    pub(crate) fn custom_tag_names(&self) -> Vec<&str> {
        self.inner
            .iter()
            .filter_map(|tag| {
                if let Tag::Custom(custom) = tag {
                    Some(custom.name())
                } else {
                    None
                }
            })
            .collect()
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

    /// Return the `TimeoutTag` if it exists.
    pub(crate) fn timeout_tag(&self) -> Option<TimeoutTag> {
        for tag in &self.inner {
            if let Tag::Timeout(timeout_tag) = tag {
                return Some(*timeout_tag);
            }
        }
        None
    }
}
