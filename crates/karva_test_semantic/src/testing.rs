use pyo3::prelude::*;

use crate::init_module;

#[cfg(test)]
#[ctor::ctor]
pub(crate) fn setup() {
    setup_module();
}

pub fn setup_module() {
    #[pymodule]
    pub(crate) fn karva(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
        init_module(py, m)?;
        Ok(())
    }
    pyo3::append_to_inittab!(karva);
}
