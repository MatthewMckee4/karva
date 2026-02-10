use std::cell::RefCell;

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

struct SnapshotContext {
    test_file: String,
    test_name: String,
    counter: u32,
}

thread_local! {
    static SNAPSHOT_CONTEXT: RefCell<Option<SnapshotContext>> = const { RefCell::new(None) };
}

/// Called by the test runner before each test to set snapshot context.
pub(crate) fn set_snapshot_context(test_file: String, test_name: String) {
    SNAPSHOT_CONTEXT.with(|ctx| {
        *ctx.borrow_mut() = Some(SnapshotContext {
            test_file,
            test_name,
            counter: 0,
        });
    });
}

/// Assert that a value matches a stored snapshot.
///
/// On first run (no existing snapshot), writes a pending `.snap.new` file.
/// On subsequent runs, compares against the existing `.snap` file.
/// If `KARVA_SNAPSHOT_UPDATE` is set, writes directly to `.snap` instead of `.snap.new`.
///
/// When `inline` is provided, the expected value lives in the test source file
/// instead of a separate `.snap` file.
#[pyfunction]
#[pyo3(signature = (value, *, inline=None))]
#[expect(clippy::needless_pass_by_value)]
pub fn assert_snapshot(py: Python<'_>, value: Py<PyAny>, inline: Option<String>) -> PyResult<()> {
    let (test_file, test_name, counter) = SNAPSHOT_CONTEXT
        .with(|ctx| {
            let mut ctx = ctx.borrow_mut();
            let snapshot_ctx = ctx.as_mut()?;
            let result = (
                snapshot_ctx.test_file.clone(),
                snapshot_ctx.test_name.clone(),
                snapshot_ctx.counter,
            );
            snapshot_ctx.counter += 1;
            Some(result)
        })
        .ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err(
                "assert_snapshot() called outside of a karva test context",
            )
        })?;

    let serialized = serialize_value(py, &value)?;

    let update_mode =
        std::env::var(EnvVars::KARVA_SNAPSHOT_UPDATE).is_ok_and(|v| v == "1" || v == "true");

    if let Some(ref inline_value) = inline {
        return handle_inline_snapshot(
            py,
            &serialized,
            inline_value,
            &test_file,
            &test_name,
            update_mode,
        );
    }

    let snapshot_name = compute_snapshot_name(&test_name, counter);

    let test_file_path = Utf8Path::new(&test_file);
    let module_name = test_file_path.file_stem().unwrap_or("unknown");

    // Sanitize `::` to `__` for filesystem compatibility (`:` is reserved on Windows)
    let fs_snapshot_name = snapshot_name.replace("::", "__");
    let snap_path = snapshot_path(test_file_path, module_name, &fs_snapshot_name);

    let relative_test_file = test_file_path
        .file_name()
        .unwrap_or(test_file_path.as_str());

    let source = if let Some(lineno) = caller_line_number(py) {
        format!("{relative_test_file}:{lineno}::{test_name}")
    } else {
        format!("{relative_test_file}::{test_name}")
    };

    let new_snapshot = SnapshotFile {
        metadata: SnapshotMetadata {
            source: Some(source),
            ..Default::default()
        },
        content: serialized.clone(),
    };

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
            "Snapshot mismatch for '{snapshot_name}'.\nSnapshot file: {snap_path}\n{diff}"
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

/// Handle an inline snapshot assertion.
fn handle_inline_snapshot(
    py: Python<'_>,
    actual: &str,
    inline_value: &str,
    test_file: &str,
    test_name: &str,
    update_mode: bool,
) -> PyResult<()> {
    let (source_file, lineno) = caller_source_info(py).ok_or_else(|| {
        pyo3::exceptions::PyRuntimeError::new_err(
            "Could not determine caller source info for inline snapshot",
        )
    })?;

    let expected = karva_snapshot::inline::dedent(inline_value);

    // Empty inline value is always treated as new/pending
    let is_empty = inline_value.is_empty();
    let matches = !is_empty && expected.trim_end() == actual.trim_end();

    if matches {
        return Ok(());
    }

    if update_mode {
        karva_snapshot::inline::rewrite_inline_snapshot(&source_file, lineno, actual).map_err(
            |e| SnapshotMismatchError::new_err(format!("Failed to update inline snapshot: {e}")),
        )?;
        return Ok(());
    }

    // Write a .snap.new with inline metadata so `karva snapshot accept` can rewrite the source
    let test_file_path = Utf8Path::new(test_file);
    let module_name = test_file_path.file_stem().unwrap_or("unknown");
    let snapshot_name = format!("{test_name}_inline_{lineno}");
    let snap_path =
        karva_snapshot::storage::snapshot_path(test_file_path, module_name, &snapshot_name);

    let relative_test_file = test_file_path
        .file_name()
        .unwrap_or(test_file_path.as_str());

    let pending_snapshot = SnapshotFile {
        metadata: SnapshotMetadata {
            source: Some(format!("{relative_test_file}:{lineno}::{test_name}")),
            inline_source: Some(source_file),
            inline_line: Some(lineno),
        },
        content: actual.to_string(),
    };

    write_pending_snapshot(&snap_path, &pending_snapshot).map_err(|e| {
        SnapshotMismatchError::new_err(format!("Failed to write pending inline snapshot: {e}"))
    })?;

    if is_empty {
        let pending = Utf8PathBuf::from(format!("{snap_path}.new"));
        return Err(SnapshotMismatchError::new_err(format!(
            "New inline snapshot for '{test_name}'.\nRun `karva snapshot accept` to accept, or re-run with `--snapshot-update`.\nPending file: {pending}"
        )));
    }

    let diff = format_diff(&expected, actual);
    Err(SnapshotMismatchError::new_err(format!(
        "Inline snapshot mismatch for '{test_name}'.\n{diff}"
    )))
}

/// Get the line number of the Python caller using `sys._getframe(0)`.
///
/// Since `assert_snapshot` is a `#[pyfunction]`, it doesn't create a Python frame,
/// so depth 0 gives the test function's frame.
fn caller_line_number(py: Python<'_>) -> Option<u32> {
    let sys = py.import("sys").ok()?;
    let frame = sys.call_method1("_getframe", (0,)).ok()?;
    frame.getattr("f_lineno").ok()?.extract::<u32>().ok()
}

/// Get both the filename and line number of the Python caller.
fn caller_source_info(py: Python<'_>) -> Option<(String, u32)> {
    let sys = py.import("sys").ok()?;
    let frame = sys.call_method1("_getframe", (0,)).ok()?;
    let lineno = frame.getattr("f_lineno").ok()?.extract::<u32>().ok()?;
    let filename = frame
        .getattr("f_code")
        .ok()?
        .getattr("co_filename")
        .ok()?
        .extract::<String>()
        .ok()?;
    Some((filename, lineno))
}

/// Compute the snapshot name based on test name and counter.
fn compute_snapshot_name(test_name: &str, counter: u32) -> String {
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

    if counter == 0 {
        format!("{base_name}{param_suffix}")
    } else {
        format!("{base_name}-{}{param_suffix}", counter + 1)
    }
}

/// Serialize a Python value to its string representation.
fn serialize_value(py: Python<'_>, value: &Py<PyAny>) -> PyResult<String> {
    let bound = value.bind(py);
    Ok(bound.str()?.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_snapshot_name_first() {
        assert_eq!(compute_snapshot_name("test_foo", 0), "test_foo");
    }

    #[test]
    fn test_compute_snapshot_name_counter() {
        assert_eq!(compute_snapshot_name("test_foo", 1), "test_foo-2");
        assert_eq!(compute_snapshot_name("test_foo", 2), "test_foo-3");
    }

    #[test]
    fn test_compute_snapshot_name_parametrized() {
        assert_eq!(
            compute_snapshot_name("test_foo(a=1, b=2)", 0),
            "test_foo(a=1, b=2)"
        );
    }
}
