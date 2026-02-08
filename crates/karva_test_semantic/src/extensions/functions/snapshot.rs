use camino::{Utf8Path, Utf8PathBuf};
use karva_snapshot::diff::format_diff;
use karva_snapshot::format::{SnapshotFile, SnapshotMetadata};
use karva_snapshot::storage::{
    read_snapshot, snapshot_path, write_pending_snapshot, write_snapshot,
};
use karva_static::EnvVars;
use pyo3::prelude::*;

pyo3::create_exception!(
    karva,
    SnapshotMismatchError,
    pyo3::exceptions::PyAssertionError
);

/// Assert that a value matches a stored snapshot.
///
/// On first run (no existing snapshot), writes a pending `.snap.new` file.
/// On subsequent runs, compares against the existing `.snap` file.
/// If `KARVA_SNAPSHOT_UPDATE` is set, writes directly to `.snap` instead of `.snap.new`.
#[pyfunction]
#[pyo3(signature = (value, *, name=None, format=None))]
#[expect(clippy::needless_pass_by_value)]
pub fn assert_snapshot(
    py: Python<'_>,
    value: Py<PyAny>,
    name: Option<String>,
    format: Option<String>,
) -> PyResult<()> {
    let karva_module = py.import("karva")?;

    let test_file: String = karva_module.getattr("_snapshot_test_file")?.extract()?;
    let test_name: String = karva_module.getattr("_snapshot_test_name")?.extract()?;
    let counter: u32 = karva_module.getattr("_snapshot_counter")?.extract()?;

    // Increment the counter for the next call
    karva_module.setattr("_snapshot_counter", counter + 1)?;

    let format_name = format.as_deref().unwrap_or("str");

    let serialized = serialize_value(py, &value, format_name)?;

    let snapshot_name = compute_snapshot_name(&test_name, name.as_deref(), counter);

    let test_file_path = Utf8Path::new(&test_file);
    let module_name = test_file_path.file_stem().unwrap_or("unknown");

    // Sanitize `::` to `__` for filesystem compatibility (`:` is reserved on Windows)
    let fs_snapshot_name = snapshot_name.replace("::", "__");
    let snap_path = snapshot_path(test_file_path, module_name, &fs_snapshot_name);

    let expression = format!("{format_name}(value)");

    let relative_test_file = test_file_path
        .file_name()
        .unwrap_or(test_file_path.as_str());

    let source = format!("{relative_test_file}::{test_name}");

    let new_snapshot = SnapshotFile {
        metadata: SnapshotMetadata {
            source: Some(source),
            expression: Some(expression),
        },
        content: serialized.clone(),
    };

    let update_mode =
        std::env::var(EnvVars::KARVA_SNAPSHOT_UPDATE).is_ok_and(|v| v == "1" || v == "true");

    if let Some(existing) = read_snapshot(&snap_path) {
        if existing.content.trim_end() == serialized.trim_end() {
            return Ok(());
        }

        // Mismatch
        if update_mode {
            write_snapshot(&snap_path, &new_snapshot).map_err(|e| {
                SnapshotMismatchError::new_err(format!("Failed to update snapshot: {e}"))
            })?;
            return Ok(());
        }

        write_pending_snapshot(&snap_path, &new_snapshot).map_err(|e| {
            SnapshotMismatchError::new_err(format!("Failed to write pending snapshot: {e}"))
        })?;

        let diff = format_diff(&existing.content, &serialized);
        return Err(SnapshotMismatchError::new_err(format!(
            "Snapshot mismatch for '{snapshot_name}'.\nSnapshot file: {snap_path}\n\n{diff}"
        )));
    }

    // No existing snapshot
    if update_mode {
        write_snapshot(&snap_path, &new_snapshot).map_err(|e| {
            SnapshotMismatchError::new_err(format!("Failed to write snapshot: {e}"))
        })?;
    } else {
        write_pending_snapshot(&snap_path, &new_snapshot).map_err(|e| {
            SnapshotMismatchError::new_err(format!("Failed to write pending snapshot: {e}"))
        })?;

        let pending = Utf8PathBuf::from(format!("{snap_path}.new"));
        return Err(SnapshotMismatchError::new_err(format!(
            "New snapshot for '{snapshot_name}'.\nRun `karva snapshot accept` to accept, or re-run with `--snapshot-update`.\nPending file: {pending}"
        )));
    }

    Ok(())
}

/// Compute the snapshot name based on test name, explicit name, and counter.
fn compute_snapshot_name(test_name: &str, explicit_name: Option<&str>, counter: u32) -> String {
    // Extract just the function name portion (before any parametrize params)
    let base_name = if let Some(paren_idx) = test_name.find('(') {
        &test_name[..paren_idx]
    } else {
        test_name
    };

    // If there are parametrize params, include them
    let param_suffix = if let Some(paren_idx) = test_name.find('(') {
        &test_name[paren_idx..]
    } else {
        ""
    };

    if let Some(explicit) = explicit_name {
        format!("{base_name}--{explicit}{param_suffix}")
    } else if counter == 0 {
        format!("{base_name}{param_suffix}")
    } else {
        format!("{base_name}-{}{param_suffix}", counter + 1)
    }
}

/// Serialize a Python value to a string using the specified format.
fn serialize_value(py: Python<'_>, value: &Py<PyAny>, format: &str) -> PyResult<String> {
    let bound = value.bind(py);
    match format {
        "str" => Ok(format!("{}\n", bound.str()?.to_string_lossy())),
        "repr" => Ok(format!("{}\n", bound.repr()?.to_string_lossy())),
        "json" => {
            let json_module = py.import("json")?;
            let kwargs = pyo3::types::PyDict::new(py);
            kwargs.set_item("indent", 2)?;
            kwargs.set_item("sort_keys", true)?;
            let result = json_module.call_method("dumps", (bound,), Some(&kwargs))?;
            Ok(format!("{}\n", result.str()?.to_string_lossy()))
        }
        _ => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Unknown snapshot format: '{format}'. Use 'str', 'repr', or 'json'."
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_snapshot_name_first() {
        assert_eq!(compute_snapshot_name("test_foo", None, 0), "test_foo");
    }

    #[test]
    fn test_compute_snapshot_name_counter() {
        assert_eq!(compute_snapshot_name("test_foo", None, 1), "test_foo-2");
        assert_eq!(compute_snapshot_name("test_foo", None, 2), "test_foo-3");
    }

    #[test]
    fn test_compute_snapshot_name_explicit() {
        assert_eq!(
            compute_snapshot_name("test_foo", Some("custom"), 0),
            "test_foo--custom"
        );
    }

    #[test]
    fn test_compute_snapshot_name_parametrized() {
        assert_eq!(
            compute_snapshot_name("test_foo(a=1, b=2)", None, 0),
            "test_foo(a=1, b=2)"
        );
    }

    #[test]
    fn test_compute_snapshot_name_parametrized_explicit() {
        assert_eq!(
            compute_snapshot_name("test_foo(a=1)", Some("custom"), 0),
            "test_foo--custom(a=1)"
        );
    }
}
