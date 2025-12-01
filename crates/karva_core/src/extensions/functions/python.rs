use pyo3::{exceptions::PyTypeError, prelude::*};

use crate::extensions::tags::{Tag, Tags, parametrize::Parametrization};

#[derive(Debug, Clone)]
#[pyclass]
pub struct Param {
    /// The values of the arguments
    ///
    /// These are used as values for the test function.
    #[pyo3(get)]
    pub(crate) values: Vec<Py<PyAny>>,

    /// Tags associated with this parametrization
    pub(crate) tags: Tags,
}

impl Param {
    pub(crate) fn new(py: Python, values: Vec<Py<PyAny>>, tags: Vec<Py<PyAny>>) -> PyResult<Self> {
        let mut new_tags = Vec::new();

        for tag in tags {
            new_tags.push(
                Tag::try_from_py_any(py, &tag)
                    .ok_or_else(|| PyTypeError::new_err("Invalid tag"))?,
            );
        }

        Ok(Self {
            values,
            tags: Tags::new(new_tags),
        })
    }

    pub(crate) fn from_parametrization(Parametrization { values, tags }: Parametrization) -> Self {
        Self { values, tags }
    }

    pub(crate) const fn values(&self) -> &Vec<Py<PyAny>> {
        &self.values
    }

    pub(crate) const fn tags(&self) -> &Tags {
        &self.tags
    }
}
