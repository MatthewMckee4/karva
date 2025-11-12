use pyo3::prelude::*;

/// Represents a skipif tag that conditionally skips tests.
///
/// The conditions will be evaluated, and if any is true, the test will be skipped.
/// Used for both pytest.mark.skipif and `karva.tags.skip_if`.
#[derive(Debug, Clone)]
pub struct SkipIfTag {
    conditions: Vec<bool>,
    reason: Option<String>,
}

impl SkipIfTag {
    pub(crate) const fn new(conditions: Vec<bool>, reason: Option<String>) -> Self {
        Self { conditions, reason }
    }

    pub(crate) fn reason(&self) -> Option<String> {
        self.reason.clone()
    }

    /// Check if the test should be skipped (if any condition is true).
    pub(crate) fn should_skip(&self) -> bool {
        self.conditions.iter().any(|&c| c)
    }

    fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let args = py_mark.getattr("args").ok()?;
        let kwargs = py_mark.getattr("kwargs").ok()?;

        // Extract all positional arguments as conditions (booleans or expressions)
        let mut conditions = Vec::new();

        if let Ok(args_tuple) = args.extract::<Bound<'_, pyo3::types::PyTuple>>() {
            for i in 0..args_tuple.len() {
                if let Ok(item) = args_tuple.get_item(i) {
                    // Try to extract as bool
                    if let Ok(bool_val) = item.extract::<bool>() {
                        conditions.push(bool_val);
                    }
                }
            }
        }

        // Need at least one condition
        if conditions.is_empty() {
            return None;
        }

        // Check for reason in kwargs
        let reason = kwargs.get_item("reason").ok().map_or_else(
            || None,
            |reason_value| reason_value.extract::<String>().ok(),
        );

        Some(Self { conditions, reason })
    }
}

impl TryFrom<&Bound<'_, PyAny>> for SkipIfTag {
    type Error = ();

    fn try_from(py_mark: &Bound<'_, PyAny>) -> Result<Self, Self::Error> {
        Self::try_from_pytest_mark(py_mark).ok_or(())
    }
}
