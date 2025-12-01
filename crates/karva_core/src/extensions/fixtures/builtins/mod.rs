pub use mock::Mock;
use pyo3::Python;

use crate::extensions::fixtures::NormalizedFixture;

mod mock;
mod temp_path;

pub fn get_builtin_fixture(py: Python<'_>, fixture_name: &str) -> Option<NormalizedFixture> {
    match fixture_name {
        _ if temp_path::is_temp_path_fixture_name(fixture_name) => {
            if let Some(path_obj) = temp_path::create_temp_dir_fixture(py) {
                return Some(NormalizedFixture::built_in(
                    fixture_name.to_string(),
                    path_obj,
                ));
            }
        }
        _ if mock::is_mock_fixture_name(fixture_name) => {
            if let Some((mock_instance, finalizer)) = mock::create_mock_fixture(py) {
                return Some(NormalizedFixture::built_in_with_finalizer(
                    fixture_name.to_string(),
                    mock_instance,
                    finalizer,
                ));
            }
        }
        _ => {}
    }

    None
}
