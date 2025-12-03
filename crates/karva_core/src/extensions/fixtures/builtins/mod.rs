pub use mock_env::MockEnv;
use pyo3::{
    prelude::*,
    types::{PyDict, PyIterator},
};

use crate::extensions::fixtures::NormalizedFixture;

mod mock_env;
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
        _ if mock_env::is_mock_env_fixture_name(fixture_name) => {
            if let Some((mock_instance, finalizer)) = mock_env::create_mock_env_fixture(py) {
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

/// Only used for builtin fixtures where we need to synthesize a fixture finalizer
pub fn create_fixture_with_finalizer<'py>(
    py: Python<'py>,
    fixture_return_value: &Py<PyAny>,
    finalizer_function: &Py<PyAny>,
) -> PyResult<Bound<'py, PyIterator>> {
    let code = r"
def _builtin_finalizer(value, finalizer):
    yield value
    finalizer()
    ";

    let locals = PyDict::new(py);

    py.run(&std::ffi::CString::new(code).unwrap(), None, Some(&locals))?;

    let generator_function = locals
        .get_item("_builtin_finalizer")?
        .expect("To find generator the function");

    let iterator = generator_function.call1((fixture_return_value, finalizer_function))?;

    let iterator = iterator.cast_into::<PyIterator>()?;

    Ok(iterator)
}
