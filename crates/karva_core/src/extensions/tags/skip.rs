use pyo3::prelude::*;

use crate::extensions::functions::SkipError;

/// Represents a test that should be skipped.
///
/// A given reason will be logged if given.
/// Can optionally have conditions that determine if the test should be skipped.
#[derive(Debug, Clone)]
pub struct SkipTag {
    conditions: Vec<bool>,
    reason: Option<String>,
}

impl SkipTag {
    pub(crate) const fn new(conditions: Vec<bool>, reason: Option<String>) -> Self {
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

    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let kwargs = py_mark.getattr("kwargs").ok()?;
        let args = py_mark.getattr("args").ok()?;

        // Extract conditions from positional arguments (if any)
        let mut conditions = Vec::new();
        if let Ok(args_tuple) = args.extract::<Bound<'_, pyo3::types::PyTuple>>() {
            for i in 0..args_tuple.len() {
                if let Ok(item) = args_tuple.get_item(i) {
                    if let Ok(bool_val) = item.extract::<bool>() {
                        conditions.push(bool_val);
                    } else if item.extract::<String>().is_ok() {
                        // This is a reason passed as positional arg (old pytest style)
                        // Skip this, we'll handle it below
                        break;
                    }
                }
            }
        }

        // Extract reason from kwargs or from first string arg
        let reason = kwargs.get_item("reason").map_or_else(
            |_| {
                if conditions.is_empty() {
                    // No boolean conditions found, check if first arg is a string reason
                    args.extract::<Bound<'_, pyo3::types::PyTuple>>()
                        .map_or(None, |args_tuple| {
                            args_tuple
                                .get_item(0)
                                .map_or(None, |first_arg| first_arg.extract::<String>().ok())
                        })
                } else {
                    None
                }
            },
            |reason| reason.extract::<String>().ok(),
        );

        Some(Self { conditions, reason })
    }
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
        && err.matches(py, skipped).unwrap_or(false)
    {
        return true;
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
