use pyo3::Python;

use crate::extensions::fixtures::FixtureGetResult;

pub(crate) mod temp_path;

pub(crate) fn get_builtin_fixture(py: Python<'_>, fixture_name: &str) -> Option<FixtureGetResult> {
    match fixture_name {
        _ if temp_path::is_temp_path_fixture_name(fixture_name) => {
            if let Some(path_obj) = temp_path::create_temp_dir(py) {
                return Some(FixtureGetResult::Single(path_obj));
            }
        }
        _ => {}
    }

    None
}
