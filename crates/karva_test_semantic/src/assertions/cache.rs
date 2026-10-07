//! Hash validated caches for assertion-rewritten Python code.
//!
//! The cache uses `CPython`'s own pyc format and the original source loader's
//! atomic writer. Cache failures are deliberately invisible to importing:
//! callers can always fall back to compiling the rewritten source.

use pyo3::prelude::*;
use pyo3::types::{PyAnyMethods, PyBytes, PyDict, PyDictMethods, PySlice};

/// Bump this when the rewritten code or the metadata reconstructed from plans
/// changes. The Python optimization level is appended separately.
const CACHE_VERSION: &str = "karvaV1";

fn cache_path<'py>(py: Python<'py>, filename: &str) -> Option<Bound<'py, PyAny>> {
    let optimize = py
        .import("sys")
        .ok()?
        .getattr("flags")
        .ok()?
        .getattr("optimize")
        .ok()?
        .extract::<u8>()
        .ok()?;
    let optimization = format!("{CACHE_VERSION}O{optimize}");
    let kwargs = PyDict::new(py);
    kwargs.set_item("optimization", optimization).ok()?;
    py.import("importlib.util")
        .ok()?
        .call_method("cache_from_source", (filename,), Some(&kwargs))
        .ok()
}

fn source_hash<'py>(py: Python<'py>, source: &Bound<'py, PyBytes>) -> Option<Bound<'py, PyAny>> {
    py.import("importlib.util")
        .ok()?
        .call_method1("source_hash", (source,))
        .ok()
}

/// Reads a valid, hash-checked transformed pyc or returns `None` on any cache
/// miss, corruption, incompatibility, or loader error.
pub(super) fn read<'py>(
    py: Python<'py>,
    loader: &Bound<'py, PyAny>,
    fullname: &str,
    filename: &str,
    source: &Bound<'py, PyBytes>,
) -> Option<Bound<'py, PyAny>> {
    let path = cache_path(py, filename)?;
    let data = loader.call_method1("get_data", (&path,)).ok()?;
    let data = data.cast::<PyBytes>().ok()?;
    let bytes = data.as_bytes();
    if bytes.len() < 16 {
        return None;
    }

    let hash = source_hash(py, source)?;
    let details = PyDict::new(py);
    details.set_item("name", fullname).ok()?;
    details.set_item("path", &path).ok()?;
    let external = py.import("importlib._bootstrap_external").ok()?;
    let flags = external
        .getattr("_classify_pyc")
        .ok()?
        .call((data, fullname, &details), None)
        .ok()?
        .extract::<u32>()
        .ok()?;
    // We only write checked hash-based pycs. Reject timestamp and unchecked
    // files even if a stale file happens to occupy our tagged cache path.
    if flags != 0b11 {
        return None;
    }
    external
        .getattr("_validate_hash_pyc")
        .ok()?
        .call((data, &hash, fullname, &details), None)
        .ok()?;

    let view = py
        .import("builtins")
        .ok()?
        .getattr("memoryview")
        .ok()?
        .call1((data,))
        .ok()?;
    let end = bytes.len().try_into().ok()?;
    let body = view.get_item(PySlice::new(py, 16, end, 1)).ok()?;
    let kwargs = PyDict::new(py);
    kwargs.set_item("name", fullname).ok()?;
    kwargs.set_item("bytecode_path", &path).ok()?;
    kwargs.set_item("source_path", filename).ok()?;
    external
        .getattr("_compile_bytecode")
        .ok()?
        .call((body,), Some(&kwargs))
        .ok()
}

/// Writes transformed code in `CPython`'s standard hash-based pyc format.
///
/// `CPython`'s `SourceFileLoader.set_data` performs the atomic write and treats
/// unwritable caches as best-effort. Any unavailable helper or loader failure
/// is swallowed so compilation remains the source of truth.
pub(super) fn write(
    py: Python<'_>,
    loader: &Bound<'_, PyAny>,
    filename: &str,
    source: &Bound<'_, PyBytes>,
    code: &Bound<'_, PyAny>,
) {
    let Ok(dont_write) = py
        .import("sys")
        .and_then(|sys| sys.getattr("dont_write_bytecode"))
        .and_then(|value| value.is_truthy())
    else {
        return;
    };
    if dont_write {
        return;
    }
    let Some(path) = cache_path(py, filename) else {
        return;
    };
    let Some(hash) = source_hash(py, source) else {
        return;
    };
    let Ok(external) = py.import("importlib._bootstrap_external") else {
        return;
    };
    let Ok(data) = external
        .getattr("_code_to_hash_pyc")
        .and_then(|function| function.call((code, &hash, true), None))
    else {
        return;
    };
    let _ = loader.call_method1("set_data", (&path, data));
}
