use pyo3::prelude::*;
use tempfile::TempDir;

pub fn is_temp_path_fixture_name(fixture_name: &str) -> bool {
    matches!(
        fixture_name,
        "tmp_path" | "tmpdir" | "temp_path" | "temp_dir"
    )
}

pub fn create_temp_dir_fixture(py: Python<'_>) -> Option<Py<PyAny>> {
    let temp_dir = TempDir::with_prefix("karva-").ok()?;

    let path_str = temp_dir.path().to_str()?.to_string();

    let _ = temp_dir.keep();

    let pathlib = py.import("pathlib").ok()?;
    let path_class = pathlib.getattr("Path").ok()?;
    let path_obj = path_class.call1((path_str,)).ok()?;

    Some(path_obj.unbind())
}
