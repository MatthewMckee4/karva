use pyo3::Python;

use crate::extensions::fixtures::NormalizedFixture;

mod temp_path;

pub fn get_builtin_fixture(py: Python<'_>, fixture_name: &str) -> Option<NormalizedFixture> {
    match fixture_name {
        _ if temp_path::is_temp_path_fixture_name(fixture_name) => {
            if let Some(path_obj) = temp_path::create_temp_dir(py) {
                return Some(NormalizedFixture::built_in(
                    py,
                    fixture_name.to_string(),
                    path_obj,
                ));
            }
        }
        _ => {}
    }

    None
}
