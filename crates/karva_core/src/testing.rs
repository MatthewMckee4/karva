use pyo3::prelude::*;

use crate::extensions::{
    fixtures::python::{FixtureFunctionDefinition, FixtureFunctionMarker, fixture_decorator},
    tags::python::{PyTag, PyTags, PyTestFunction},
};

#[cfg(test)]
#[ctor::ctor]
pub fn setup() {
    setup_module();
}

pub fn setup_module() {
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
    pyo3::append_to_inittab!(karva);
}
