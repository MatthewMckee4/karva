//! Reads standard-library mock assertion frames only after an uncaught failure.
//!
//! The mock library remains untouched, including when user code catches its
//! assertions. Diffs use already-matched call arguments and the existing safe
//! value renderer rather than calling user equality or representation methods.

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

use super::{focused_diff, safe_repr};

/// Terminal mock assertion frames and optional last-call argument diagnostics.
pub(crate) struct MockFailure {
    /// Number of trailing library frames to omit from source diagnostics.
    pub(crate) hidden_frames: usize,

    /// Positional and keyword differences, absent for count or call-list errors.
    pub(crate) explanation: Option<String>,
}

/// Recognizes only failures raised inside unittest.mock assertion helpers.
pub(crate) fn mock_failure(py: Python<'_>, error: &PyErr) -> Option<MockFailure> {
    let mut traceback = error.traceback(py)?.into_any();
    let mut failure = MockFailure {
        hidden_frames: 0,
        explanation: None,
    };
    while !traceback.is_none() {
        let frame = traceback.getattr("tb_frame").ok()?;
        let globals = frame
            .getattr("f_globals")
            .ok()?
            .cast_into::<PyDict>()
            .ok()?;
        let module = globals
            .get_item("__name__")
            .ok()?
            .and_then(|value| value.extract::<String>().ok());
        let name = frame
            .getattr("f_code")
            .ok()?
            .getattr("co_name")
            .ok()?
            .extract::<String>()
            .ok()?;
        if module.as_deref() == Some("unittest.mock") && name.starts_with("assert_") {
            failure.hidden_frames += 1;
            if matches!(name.as_str(), "assert_called_with" | "assert_awaited_with") {
                failure.explanation = call_difference(&frame);
            }
        } else {
            failure.hidden_frames = 0;
            failure.explanation = None;
        }
        traceback = traceback.getattr("tb_next").ok()?;
    }
    (failure.hidden_frames > 0).then_some(failure)
}

/// Uses the library's signature-normalized expected/actual tuples when present.
fn call_difference(frame: &Bound<'_, PyAny>) -> Option<String> {
    let locals = frame.getattr("f_locals").ok()?;
    let expected = locals
        .get_item("expected")
        .ok()?
        .cast_into::<PyTuple>()
        .ok()?;
    let actual = locals
        .get_item("actual")
        .ok()?
        .cast_into::<PyTuple>()
        .ok()?;
    let expected_start = expected.len().checked_sub(2)?;
    let actual_start = actual.len().checked_sub(2)?;
    let mut lines = vec!["Mock call arguments differ:".to_string()];
    for (offset, label) in [(0, "Positional arguments"), (1, "Keyword arguments")] {
        let expected = expected.get_item(expected_start + offset).ok()?;
        let actual = actual.get_item(actual_start + offset).ok()?;
        let differences = focused_diff(&actual, &expected);
        if !differences.is_empty() {
            lines.push(format!("{label}:"));
            lines.push(format!("  Actual: {}", safe_repr(&actual)));
            lines.push(format!("  Expected: {}", safe_repr(&expected)));
            lines.extend(differences);
        }
    }
    (lines.len() > 1).then(|| lines.join("\n"))
}
