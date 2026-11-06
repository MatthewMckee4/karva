//! Python logging capture independent of stdout/stderr redirection.

use karva_diagnostic::CapturedLog;
use pyo3::intern;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::PyList;

/// Runner-owned handler installed for one setup/call/teardown attempt.
pub struct LogCapture(Py<PyAny>);

impl LogCapture {
    pub(super) fn start(py: Python<'_>) -> PyResult<Self> {
        static CLASS: PyOnceLock<Py<PyAny>> = PyOnceLock::new();
        let class = CLASS.get_or_try_init(py, || {
            Ok::<_, PyErr>(
                py.import("karva._logging")?
                    .getattr("_LogCapture")?
                    .unbind(),
            )
        })?;
        let handler = class.bind(py).call0()?;
        Ok(Self(handler.unbind()))
    }

    pub(super) fn set_phase(&self, py: Python<'_>, phase: &str) -> PyResult<()> {
        self.0.bind(py).setattr(intern!(py, "phase"), phase)
    }

    /// Removes the handler before converting retained records to IPC data.
    pub(super) fn finish(self, py: Python<'_>, attempt: u32) -> PyResult<Vec<CapturedLog>> {
        let handler = self.0.bind(py);
        handler.call_method0(intern!(py, "finish"))?;
        handler
            .getattr(intern!(py, "captured"))?
            .cast::<PyList>()?
            .iter()
            .map(|record| {
                Ok(CapturedLog {
                    attempt,
                    phase: record.get_item(0)?.extract()?,
                    timestamp: record.get_item(1)?.extract()?,
                    logger: record.get_item(2)?.extract()?,
                    level: record.get_item(3)?.extract()?,
                    level_name: record.get_item(4)?.extract()?,
                    message: record.get_item(5)?.extract()?,
                    pathname: record.get_item(6)?.extract()?,
                    lineno: record.get_item(7)?.extract()?,
                    exception: record.get_item(8)?.extract()?,
                })
            })
            .collect()
    }
}
