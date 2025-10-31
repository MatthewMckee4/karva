use pyo3::prelude::*;

/// Represents a test that should be skipped.
///
/// A given reason will be logged if given.
#[derive(Debug, Clone)]
pub struct SkipTag {
    reason: Option<String>,
}

impl SkipTag {
    pub(crate) const fn new(reason: Option<String>) -> Self {
        Self { reason }
    }

    pub(crate) fn reason(&self) -> Option<String> {
        self.reason.clone()
    }

    fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let kwargs = py_mark.getattr("kwargs").ok()?;

        if let Ok(reason) = kwargs.get_item("reason") {
            if let Ok(reason_str) = reason.extract::<String>() {
                return Some(Self {
                    reason: Some(reason_str),
                });
            }
        }

        let args = py_mark.getattr("args").ok()?;

        if let Ok(args_tuple) = args.extract::<(String,)>() {
            return Some(Self {
                reason: Some(args_tuple.0),
            });
        }

        Some(Self { reason: None })
    }
}

impl TryFrom<&Bound<'_, PyAny>> for SkipTag {
    type Error = ();

    fn try_from(py_mark: &Bound<'_, PyAny>) -> Result<Self, Self::Error> {
        Self::try_from_pytest_mark(py_mark).ok_or(())
    }
}
