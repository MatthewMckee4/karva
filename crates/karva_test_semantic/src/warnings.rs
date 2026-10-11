//! Python warning capture spanning collection or one setup/call/teardown attempt.

use karva_diagnostic::{Diagnostic, Severity, TestWarning};
use pyo3::prelude::*;

use crate::Context;

/// An entered Python capture window that must be finished after user code.
pub(super) struct WarningCapture(Py<PyAny>);

impl WarningCapture {
    /// Validates filters before entering a window with isolated warning state.
    pub(super) fn start(py: Python<'_>, filters: &[String]) -> PyResult<Self> {
        let capture = py
            .import("karva._warnings")?
            .getattr("WarningCapture")?
            .call1((filters.to_vec(),))?;
        capture.call_method0("__enter__")?;
        Ok(Self(capture.unbind()))
    }

    /// Restores Python state and forwards every unconsumed warning over IPC.
    pub(super) fn finish(
        self,
        py: Python<'_>,
        context: &Context<'_>,
        test: Option<&str>,
        attempt: Option<u32>,
    ) -> PyResult<()> {
        let records: Vec<(String, String, String, u32)> =
            self.0.call_method0(py, "finish")?.extract(py)?;
        for (message, category, filename, lineno) in records {
            let filename = camino::Utf8Path::new(&filename)
                .strip_prefix(context.cwd())
                .map_or_else(|_| filename.clone(), ToString::to_string);
            context.report_warning(TestWarning {
                category,
                message,
                filename,
                lineno,
                test: test.map(ToString::to_string),
                attempt,
            });
        }
        Ok(())
    }
}

/// Warning infrastructure failures must fail the run rather than hide warnings.
pub(super) fn warning_capture_error(error: &PyErr) -> Diagnostic {
    Diagnostic::new("warning-policy", Severity::Error, error.to_string())
}
