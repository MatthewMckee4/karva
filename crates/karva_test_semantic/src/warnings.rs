//! Python warning capture spanning collection or one setup/call/teardown attempt.

use karva_diagnostic::{Diagnostic, Severity, TestWarning};
use pyo3::exceptions::PyAttributeError;
use pyo3::intern;
use pyo3::prelude::*;
use pyo3::sync::PyOnceLock;
use pyo3::types::{PyDict, PyList, PyTuple};

use crate::Context;

/// An entered Python capture window that must be finished after user code.
pub struct WarningCapture(Py<PyAny>);

/// Private capture implementation retained for the embedded interpreter.
static CAPTURE_CLASS: PyOnceLock<Py<PyAny>> = PyOnceLock::new();

impl WarningCapture {
    /// Validates filters before entering a window with isolated warning state.
    pub(super) fn start<'a>(
        py: Python<'_>,
        filters: impl IntoIterator<Item = &'a String>,
    ) -> PyResult<Self> {
        let class = CAPTURE_CLASS.get_or_try_init(py, || {
            Ok::<_, PyErr>(
                py.import("karva._warnings")?
                    .getattr("WarningCapture")?
                    .unbind(),
            )
        })?;
        let filters = PyList::new(py, filters)?;
        let capture = class.call1(py, (filters, Py::new(py, WarningRegistries::default())?))?;
        capture.call_method0(py, "__enter__")?;
        Ok(Self(capture))
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

/// Isolates suppression dictionaries without interpreting a Python loop for every
/// imported module on every test attempt. New registries are removed even when
/// their warnings were ignored or their contents were assigned directly.
#[pyclass]
#[derive(Default)]
struct WarningRegistries {
    /// Original dictionary identities and contents, retained until restoration.
    saved: Vec<SavedRegistry>,
}

/// One registry that existed before the capture window.
struct SavedRegistry {
    namespace: Py<PyDict>,

    original: Py<PyAny>,

    contents: Py<PyAny>,
}

#[pymethods]
impl WarningRegistries {
    /// Clears suppression while keeping original objects alive for restoration.
    fn start(&mut self, py: Python<'_>) -> PyResult<()> {
        let modules = module_snapshot(py)?;
        let key = intern!(py, "__warningregistry__");
        for module in modules.iter_borrowed() {
            if let Some(namespace) = module_namespace(&module)?
                && let Some(registry) = namespace.get_item(key)?
                && registry.is_instance_of::<PyDict>()
            {
                self.saved.push(SavedRegistry {
                    namespace: namespace.unbind(),
                    original: registry.clone().unbind(),
                    contents: registry.call_method0("copy")?.unbind(),
                });
                registry.call_method0("clear")?;
            }
        }
        Ok(())
    }

    /// Removes registries created inside the window and restores saved contents.
    fn finish(&mut self, py: Python<'_>) -> PyResult<()> {
        let modules = module_snapshot(py)?;
        let key = intern!(py, "__warningregistry__");
        for module in modules.iter_borrowed() {
            if let Some(namespace) = module_namespace(&module)?
                && namespace.contains(key)?
            {
                namespace.del_item(key)?;
            }
        }
        for saved in self.saved.drain(..) {
            let original = saved.original.bind(py);
            original.call_method0("clear")?;
            original.call_method1("update", (saved.contents,))?;
            saved.namespace.bind(py).set_item(key, original)?;
        }
        Ok(())
    }
}

/// Freezes iteration before registry callbacks can change the imported modules.
fn module_snapshot(py: Python<'_>) -> PyResult<Bound<'_, PyTuple>> {
    let modules = py.import("sys")?.getattr("modules")?;
    py.get_type::<PyTuple>()
        .call1((modules.call_method0("values")?,))?
        .cast_into()
        .map_err(Into::into)
}

/// Uses module dictionaries directly, preserving support for non-module entries
/// in `sys.modules` that expose a dictionary namespace.
fn module_namespace<'py>(module: &Bound<'py, PyAny>) -> PyResult<Option<Bound<'py, PyDict>>> {
    if let Ok(module) = module.cast::<PyModule>() {
        return Ok(Some(module.dict()));
    }
    match module.getattr(intern!(module.py(), "__dict__")) {
        Ok(namespace) => Ok(namespace.cast_into::<PyDict>().ok()),
        Err(error) if error.is_instance_of::<PyAttributeError>(module.py()) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Warning infrastructure failures must fail the run rather than hide warnings.
pub fn warning_capture_error(error: &PyErr) -> Diagnostic {
    Diagnostic::new("warning-policy", Severity::Error, error.to_string())
}
