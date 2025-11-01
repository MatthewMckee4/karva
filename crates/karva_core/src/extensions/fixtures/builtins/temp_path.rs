use pyo3::prelude::*;
use tempfile::TempDir;

pub fn is_temp_path_fixture_name(fixture_name: &str) -> bool {
    match fixture_name {
        // pytest names
        "tmp_path" | "tmpdir" |
        // karva names
        "temp_path" | "temp_dir" => true,
        _ => false,
    }
}

pub fn create_temp_dir(py: Python<'_>) -> Option<Py<PyAny>> {
    let temp_dir = TempDir::with_prefix("karva-").ok()?;

    let path_str = temp_dir.path().to_str()?.to_string();

    let _ = temp_dir.keep();

    let pathlib = py.import("pathlib").ok()?;
    let path_class = pathlib.getattr("Path").ok()?;
    let path_obj = path_class.call1((path_str,)).ok()?;

    Some(path_obj.unbind())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_directory() {
        Python::attach(|py| {
            let path = create_temp_dir(py).unwrap();

            let pathlib = py.import("pathlib").unwrap();
            let path_class = pathlib.getattr("Path").unwrap();
            assert!(path.bind(py).is_instance(&path_class).unwrap());

            let exists_method = path.getattr(py, "exists").unwrap();
            let exists = exists_method
                .call0(py)
                .unwrap()
                .extract::<bool>(py)
                .unwrap();
            assert!(exists);
        });
    }
}
