use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

use super::parse_pytest_mark_args;
use crate::extensions::functions::SkipError;

/// Represents a test marked to be skipped.
///
/// Supports unconditional skipping and conditional skipping via `skipif`.
/// An optional reason can be provided for documentation.
#[derive(Debug, Clone)]
pub struct SkipTag {
    /// Boolean conditions; test is skipped if any is true (or if empty).
    conditions: Vec<bool>,

    /// Optional explanation for why the test is skipped.
    reason: Option<String>,
}

impl SkipTag {
    pub(crate) fn new(conditions: Vec<bool>, reason: Option<String>) -> Self {
        Self { conditions, reason }
    }

    pub(crate) fn reason(&self) -> Option<String> {
        self.reason.clone()
    }

    /// Check if the test should be skipped.
    /// If there are no conditions, always skip.
    /// If there are conditions, skip only if any condition is true.
    pub(crate) fn should_skip(&self) -> bool {
        if self.conditions.is_empty() {
            true
        } else {
            self.conditions.iter().any(|&c| c)
        }
    }

    pub(crate) fn try_from_pytest_mark(
        py_mark: &Bound<'_, PyAny>,
        globals: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<Self>> {
        let name = py_mark.getattr("name")?.extract::<String>()?;

        if name == "skip" {
            return parse_pytest_skip_mark(py_mark).map(Some);
        }

        let parsed = parse_pytest_mark_args(py_mark, globals)?;
        let kwargs = py_mark.getattr("kwargs")?;
        let reason =
            if let Ok(reason_item) = kwargs.get_item("reason") {
                Some(reason_item.extract::<String>().map_err(|_| {
                    PyValueError::new_err("pytest skipif mark reason must be a string")
                })?)
            } else {
                parsed.reason
            };

        Ok(Some(Self {
            conditions: parsed.conditions,
            reason,
        }))
    }
}

fn parse_pytest_skip_mark(py_mark: &Bound<'_, PyAny>) -> PyResult<SkipTag> {
    let kwargs = py_mark.getattr("kwargs")?;
    let args = py_mark.getattr("args")?;

    let reason =
        if let Ok(reason_item) = kwargs.get_item("reason") {
            Some(
                reason_item.extract::<String>().map_err(|_| {
                    PyValueError::new_err("pytest skip mark reason must be a string")
                })?,
            )
        } else {
            let args = args.extract::<Bound<'_, PyTuple>>()?;
            match args.get_item(0) {
                Ok(reason_item) => Some(reason_item.extract::<String>().map_err(|_| {
                    PyValueError::new_err("pytest skip mark reason must be a string")
                })?),
                Err(_) => None,
            }
        };

    Ok(SkipTag {
        conditions: Vec::new(),
        reason,
    })
}

/// Check if the given `PyErr` is a skip exception.
pub fn is_skip_exception(py: Python<'_>, err: &PyErr) -> bool {
    // Check for karva.SkipError
    if err.is_instance_of::<SkipError>(py) {
        return true;
    }

    // Check for pytest skip exception
    if let Ok(pytest_module) = py.import("_pytest.outcomes")
        && let Ok(skipped) = pytest_module.getattr("Skipped")
    {
        return match err.matches(py, &skipped) {
            Ok(is_skipped) => is_skipped,
            Err(match_err) => {
                tracing::warn!("Failed to classify pytest skip exception: {match_err}");
                false
            }
        };
    }

    false
}

/// Extract the skip reason from a skip exception.
pub fn extract_skip_reason(py: Python<'_>, err: &PyErr) -> Option<String> {
    let value = err.value(py);

    // Try to get the first argument (the message)
    if let Ok(args) = value.getattr("args")
        && let Ok(tuple) = args.cast::<pyo3::types::PyTuple>()
        && let Ok(first_arg) = tuple.get_item(0)
        && let Ok(message) = first_arg.extract::<String>()
    {
        if message.is_empty() {
            return None;
        }
        return Some(message);
    }

    None
}
