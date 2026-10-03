use std::sync::Arc;

use pyo3::exceptions::{PyBaseException, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple, PyType};

use super::parse_pytest_mark_args;

/// Represents a test marked as expected to fail (xfail).
///
/// Only a matching test-body exception counts as an expected failure.
/// If the test passes unexpectedly, it counts as failed.
/// Supports conditional xfail via boolean conditions.
#[derive(Debug, Clone)]
pub struct ExpectFailTag {
    /// Boolean conditions; test is xfailed if any is true (or if empty).
    conditions: Vec<bool>,

    /// Optional explanation for why failure is expected.
    reason: Option<String>,

    /// Validated exception class or tuple, shared with cloned variant settings.
    raises: Option<Arc<Py<PyAny>>>,
}

impl ExpectFailTag {
    pub(super) fn new(
        conditions: Vec<bool>,
        reason: Option<String>,
        raises: Option<Arc<Py<PyAny>>>,
    ) -> Self {
        Self {
            conditions,
            reason,
            raises,
        }
    }

    pub(crate) fn reason(&self) -> Option<String> {
        self.reason.clone()
    }

    /// Uses Python exception subclass semantics after collection validated the policy.
    pub(crate) fn matches(&self, py: Python<'_>, error: &PyErr) -> bool {
        self.should_expect_fail()
            && self
                .raises
                .as_ref()
                .is_none_or(|raises| error.matches(py, raises.bind(py)).unwrap_or(false))
    }

    /// Check if the test should be expected to fail.
    /// If there are no conditions, always expect fail.
    /// If there are conditions, expect fail only if any condition is true.
    pub(crate) fn should_expect_fail(&self) -> bool {
        if self.conditions.is_empty() {
            true
        } else {
            self.conditions.iter().any(|&c| c)
        }
    }

    pub(super) fn try_from_pytest_mark(
        py_mark: &Bound<'_, PyAny>,
        globals: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<Self>> {
        let mut parsed = parse_pytest_mark_args(py_mark, globals)?;
        let kwargs = py_mark.getattr("kwargs")?;
        if let Ok(condition) = kwargs.get_item("condition") {
            parsed.conditions.push(condition.is_truthy()?);
            parsed.requires_reason = true;
        }
        let reason = if let Ok(reason_item) = kwargs.get_item("reason") {
            Some(reason_item.str()?.to_string_lossy().into_owned())
        } else {
            parsed.reason
        };

        if parsed.requires_reason && reason.is_none() {
            return Err(PyValueError::new_err(
                "pytest xfail mark requires a reason when using boolean conditions",
            ));
        }

        let raises = kwargs
            .get_item("raises")
            .ok()
            .filter(|value| !value.is_none());
        if let Some(raises) = &raises {
            validate_raises(raises)?;
        }
        Ok(Some(Self {
            conditions: parsed.conditions,
            reason,
            raises: raises.map(|raises| Arc::new(raises.unbind())),
        }))
    }
}

/// Rejects malformed policies while collecting the declaration, before running tests.
pub(super) fn validate_raises(raises: &Bound<'_, PyAny>) -> PyResult<()> {
    fn validate_class(value: &Bound<'_, PyAny>) -> PyResult<()> {
        if let Ok(class) = value.cast::<PyType>()
            && class.is_subclass_of::<PyBaseException>()?
        {
            return Ok(());
        }
        Err(PyTypeError::new_err(
            "expect_fail raises must be an exception class or a tuple of exception classes",
        ))
    }
    if let Ok(classes) = raises.cast::<PyTuple>() {
        for class in classes.iter() {
            validate_class(&class)?;
        }
        Ok(())
    } else {
        validate_class(raises)
    }
}
