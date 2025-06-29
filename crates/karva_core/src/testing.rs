use std::sync::Once;

use pyo3::prelude::*;

use crate::{
    fixture::python::{FixtureFunctionDefinition, FixtureFunctionMarker, fixture_decorator},
    tag::python::{PyTag, PyTags, PyTestFunction},
};

static INIT: Once = Once::new();

pub fn setup() {
    INIT.call_once(|| {
        #[pymodule]
        pub fn karva(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
            m.add_function(wrap_pyfunction!(fixture_decorator, m)?)?;
            m.add_class::<FixtureFunctionMarker>()?;
            m.add_class::<FixtureFunctionDefinition>()?;
            m.add_class::<PyTag>()?;
            m.add_class::<PyTags>()?;
            m.add_class::<PyTestFunction>()?;
            Ok(())
        }
        unsafe {
            while pyo3::ffi::Py_IsInitialized() != 0 {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
        pyo3::append_to_inittab!(karva);
    });
}
