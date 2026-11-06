//! Python logging capture independent of stdout/stderr redirection.

use karva_diagnostic::CapturedLog;
use pyo3::prelude::*;

/// Runner-owned handler installed for one setup/call/teardown attempt.
pub struct LogCapture(Py<PyAny>);

impl LogCapture {
    pub(crate) fn start(py: Python<'_>) -> PyResult<Self> {
        let handler = py
            .import("karva._logging")?
            .getattr("_LogCapture")?
            .call0()?;
        handler.call_method0("start")?;
        Ok(Self(handler.unbind()))
    }

    pub(crate) fn set_phase(&self, py: Python<'_>, phase: &str) -> PyResult<()> {
        self.0.bind(py).setattr("phase", phase)
    }

    /// Removes the handler before converting retained records to IPC data.
    pub(crate) fn finish(self, py: Python<'_>, attempt: u32) -> PyResult<Vec<CapturedLog>> {
        let handler = self.0.bind(py);
        handler.call_method0("finish")?;
        handler
            .getattr("captured")?
            .try_iter()?
            .map(|record| {
                let record = record?;
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
