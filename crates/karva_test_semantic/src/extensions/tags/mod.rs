//! Registration, inheritance, and execution hooks for Karva test tags.

use std::{collections::HashSet, ffi::CString, ops::Deref, sync::Arc};

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use ruff_python_ast::StmtFunctionDef;

use crate::extensions::tags::python::{PyTag, PyTags, PyTestFunction};

pub mod custom;
pub mod expect_fail;
pub mod fail_slow;
pub mod parametrize;
pub mod python;
pub mod skip;
pub mod timeout;
mod use_fixtures;
pub(crate) mod validation;

use custom::CustomTag;
use expect_fail::ExpectFailTag;
use fail_slow::FailSlowTag;
use parametrize::{InvalidParametrizeError, ParameterPlan, ParametrizeTag};
use skip::SkipTag;
use timeout::TimeoutTag;
use use_fixtures::UseFixturesTag;

/// Parsed conditions and reason extracted from a pytest mark's args and kwargs.
///
/// Used by both `SkipTag` and `ExpectFailTag` which share identical parsing logic.
pub struct ParsedMarkArgs {
    /// Evaluated positional conditions; empty means an unconditional mark.
    pub conditions: Vec<bool>,

    /// Explicit reason or generated description of a true string condition.
    pub reason: Option<String>,

    /// Whether pytest compatibility requires an explicit reason.
    pub requires_reason: bool,
}

/// Extract conditions and reason from a pytest mark object.
///
/// Pytest marks store truthy/falsy conditions as positional args and an optional
/// `reason` as a keyword argument. String conditions are evaluated with the
/// owning function's globals when available.
pub fn parse_pytest_mark_args(
    py_mark: &Bound<'_, PyAny>,
    globals: Option<&Bound<'_, PyDict>>,
) -> PyResult<ParsedMarkArgs> {
    let kwargs = py_mark.getattr("kwargs")?;
    let args = py_mark.getattr("args")?;

    let mut conditions = Vec::new();
    let mut condition_reason = None;
    let mut requires_reason = false;
    if let Ok(args_tuple) = args.extract::<Bound<'_, pyo3::types::PyTuple>>() {
        for i in 0..args_tuple.len() {
            let item = args_tuple.get_item(i)?;
            if let Ok(expression) = item.extract::<String>() {
                let Some(globals) = globals else {
                    break;
                };
                let condition = evaluate_pytest_condition(&expression, globals)?;
                if condition && condition_reason.is_none() {
                    condition_reason = Some(format!("condition: {expression}"));
                }
                conditions.push(condition);
                continue;
            }
            requires_reason = true;
            conditions.push(item.is_truthy()?);
        }
    }

    let reason = if let Ok(reason_item) = kwargs.get_item("reason") {
        reason_item.extract::<String>().ok()
    } else if conditions.is_empty() {
        // Fall back to first positional arg as reason when no globals were
        // available for evaluating legacy pytest string conditions.
        args.extract::<Bound<'_, pyo3::types::PyTuple>>()
            .ok()
            .and_then(|t| t.get_item(0).ok())
            .and_then(|a| a.extract::<String>().ok())
    } else {
        condition_reason
    };

    Ok(ParsedMarkArgs {
        conditions,
        reason,
        requires_reason,
    })
}

fn evaluate_pytest_condition(expression: &str, globals: &Bound<'_, PyDict>) -> PyResult<bool> {
    let expression = CString::new(expression).map_err(|_| {
        PyValueError::new_err("pytest mark string condition cannot contain a null byte")
    })?;
    globals
        .py()
        .eval(expression.as_c_str(), Some(globals), None)?
        .is_truthy()
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
    FailSlow(FailSlowTag),
    Custom(CustomTag),
}

impl Tag {
    fn name(&self) -> &str {
        match self {
            Self::Parametrize(_) => "parametrize",
            Self::UseFixtures(_) => "use_fixtures",
            Self::Skip(_) => "skip",
            Self::ExpectFail(_) => "expect_fail",
            Self::Timeout(_) => "timeout",
            Self::FailSlow(_) => "fail_slow",
            Self::Custom(custom) => custom.name(),
        }
    }

    /// Converts a Pytest mark into an Karva Tag.
    ///
    /// This is used to allow Pytest marks to be used as Karva tags.
    fn try_from_pytest_mark(
        py_mark: &Bound<'_, PyAny>,
        globals: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<Self>> {
        let Some(name) = py_mark
            .getattr("name")
            .ok()
            .and_then(|name| name.extract::<String>().ok())
        else {
            return Ok(None);
        };

        match name.as_str() {
            "parametrize" => ParametrizeTag::try_from_pytest_mark(py_mark, globals)
                .map(|tag| tag.map(Self::Parametrize)),
            "usefixtures" => {
                UseFixturesTag::try_from_pytest_mark(py_mark).map(|tag| tag.map(Self::UseFixtures))
            }
            "skip" | "skipif" => {
                SkipTag::try_from_pytest_mark(py_mark, globals).map(|tag| tag.map(Self::Skip))
            }
            "xfail" => ExpectFailTag::try_from_pytest_mark(py_mark, globals)
                .map(|tag| tag.map(Self::ExpectFail)),
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
    pub(super) fn try_from_py_any(py: Python, py_any: &Py<PyAny>) -> Option<Self> {
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
    fn from_karva_tag<T>(py: Python, py_tag: T) -> Self
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
            PyTag::FailSlow { seconds } => Self::FailSlow(FailSlowTag::new(*seconds)),
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

/// Runtime tag policy compiled from decorators and parameter-specific marks.
#[derive(Clone, Debug, Default)]
pub struct RuntimeTags {
    skip: SkipPolicy,
    expect_fail: Option<ExpectFailTag>,
    timeout: Option<TimeoutTag>,
    fail_slow: Option<FailSlowTag>,
    names: Vec<String>,
}

#[derive(Clone, Debug, Default)]
enum SkipPolicy {
    #[default]
    Run,
    Skip(Option<String>),
}

impl RuntimeTags {
    fn from_tags(tags: &Tags) -> Self {
        let mut runtime = Self::default();
        runtime.extend(tags);
        runtime
    }

    pub(crate) fn extend(&mut self, tags: &Tags) {
        for tag in &tags.inner {
            self.names.push(tag.name().to_string());
            match tag {
                Tag::Skip(skip) if matches!(self.skip, SkipPolicy::Run) && skip.should_skip() => {
                    self.skip = SkipPolicy::Skip(skip.reason());
                }
                Tag::ExpectFail(expect_fail) if self.expect_fail.is_none() => {
                    self.expect_fail = Some(expect_fail.clone());
                }
                Tag::Timeout(timeout) if self.timeout.is_none() => self.timeout = Some(*timeout),
                Tag::FailSlow(fail_slow) if self.fail_slow.is_none() => {
                    self.fail_slow = Some(*fail_slow);
                }
                _ => {}
            }
        }
    }

    pub(crate) fn should_skip(&self) -> (bool, Option<String>) {
        match &self.skip {
            SkipPolicy::Run => (false, None),
            SkipPolicy::Skip(reason) => (true, reason.clone()),
        }
    }

    pub(crate) fn tag_names(&self) -> Vec<&str> {
        self.names.iter().map(String::as_str).collect()
    }

    pub(crate) fn expect_fail_tag(&self) -> Option<ExpectFailTag> {
        self.expect_fail.clone()
    }

    pub(crate) fn timeout_tag(&self) -> Option<TimeoutTag> {
        self.timeout
    }

    pub(crate) fn fail_slow_tag(&self) -> Option<FailSlowTag> {
        self.fail_slow
    }
}

/// Static tag metadata and lazy parameter matrix compiled for one test.
pub struct CompiledTags {
    parameter_names: HashSet<String>,
    required_fixtures: Vec<String>,
    parameters: ParameterPlan,
    runtime: RuntimeTags,
}

impl CompiledTags {
    pub(crate) fn new(tags: &Tags) -> Self {
        let mut parameter_names = HashSet::new();
        let mut required_fixtures = Vec::new();
        let mut dimensions = Vec::new();

        for tag in &tags.inner {
            match tag {
                Tag::Parametrize(parametrize) => {
                    parameter_names.extend(parametrize.names().iter().cloned());
                    dimensions.push(parametrize.each_arg_value());
                }
                Tag::UseFixtures(use_fixtures) => {
                    required_fixtures.extend(use_fixtures.fixture_names().iter().cloned());
                }
                _ => {}
            }
        }

        Self {
            parameter_names,
            required_fixtures,
            parameters: ParameterPlan::new(dimensions),
            runtime: RuntimeTags::from_tags(tags),
        }
    }

    pub(crate) fn parameter_names(&self) -> HashSet<&str> {
        self.parameter_names.iter().map(String::as_str).collect()
    }

    pub(crate) fn required_fixtures(&self) -> &[String] {
        &self.required_fixtures
    }

    pub(crate) fn into_runtime(self) -> (ParameterPlan, RuntimeTags) {
        (self.parameters, self.runtime)
    }
}

impl Tags {
    pub(super) fn new(tags: Vec<Tag>) -> Self {
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

        let bound_function = py_function.bind(py);
        let globals = bound_function
            .getattr("__globals__")
            .ok()
            .and_then(|globals| globals.cast_into::<PyDict>().ok());
        if let Ok(marks) = py_function.getattr(py, "pytestmark")
            && let Some(tags) = Self::from_pytest_marks(py, &marks, globals.as_ref())?
        {
            return Ok(tags);
        }

        Ok(Self::default())
    }

    pub(crate) fn from_pytest_marks(
        py: Python<'_>,
        marks: &Py<PyAny>,
        globals: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<Self>> {
        let mut tags = Vec::new();
        if let Ok(marks_list) = marks.extract::<Vec<Bound<'_, PyAny>>>(py) {
            for mark in marks_list {
                if let Some(tag) = Tag::try_from_pytest_mark(&mark, globals)? {
                    tags.push(tag);
                }
            }
        } else if let Some(tag) = Tag::try_from_pytest_mark(marks.bind(py), globals)? {
            tags.push(tag);
        }
        Ok(Some(Self { inner: tags }))
    }

    pub(crate) fn validate_parametrize(
        &self,
        function: &StmtFunctionDef,
    ) -> Result<(), InvalidParametrizeError> {
        let function_parameter_names = function
            .parameters
            .iter_non_variadic_params()
            .map(|parameter| parameter.parameter.name.as_str())
            .collect();
        let mut seen_names = HashSet::new();

        for tag in &self.inner {
            if let Tag::Parametrize(parametrize) = tag {
                parametrize.validate(&function_parameter_names, &mut seen_names)?;
            }
        }

        for tag in &self.inner {
            if let Tag::Parametrize(parametrize) = tag {
                for params in parametrize.each_arg_value() {
                    if params.id().is_some_and(str::is_empty) {
                        return Err(InvalidParametrizeError::EmptyId);
                    }
                }
            }
        }

        Ok(())
    }
}
