use pyo3::prelude::*;

/// Represents a test that is expected to fail.
///
/// If the test fails, it will be reported as passed (expected failure).
/// If the test passes, it will be reported as failed (unexpected pass).
/// Can optionally have conditions that determine if the test should be expected to fail.
#[derive(Debug, Clone)]
pub struct ExpectFailTag {
    conditions: Vec<bool>,
    reason: Option<String>,
}

impl ExpectFailTag {
    pub(crate) const fn new(conditions: Vec<bool>, reason: Option<String>) -> Self {
        Self { conditions, reason }
    }

    pub(crate) fn reason(&self) -> Option<String> {
        self.reason.clone()
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
                        // This is a reason passed as positional arg
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
