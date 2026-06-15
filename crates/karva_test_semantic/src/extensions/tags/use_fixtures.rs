use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyTuple;

/// Represents required fixtures that should be called before a test function is run.
///
/// These fixtures are not specified as arguments as the function does not directly need them.
/// But they are still called.
#[derive(Debug, Clone)]
pub struct UseFixturesTag {
    /// The names of the fixtures to be called.
    fixture_names: Vec<String>,
}

impl UseFixturesTag {
    pub(crate) fn new(fixture_names: Vec<String>) -> Self {
        Self { fixture_names }
    }

    pub(crate) fn fixture_names(&self) -> &[String] {
        &self.fixture_names
    }

    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> PyResult<Option<Self>> {
        let args = py_mark.getattr("args")?;
        let tuple = args.extract::<Bound<'_, PyTuple>>()?;
        let fixture_names = tuple
            .iter()
            .map(|item| {
                item.extract::<String>().map_err(|_| {
                    PyValueError::new_err(
                        "pytest usefixtures mark arguments must be fixture name strings",
                    )
                })
            })
            .collect::<PyResult<Vec<_>>>()?;
        Ok(Some(Self::new(fixture_names)))
    }
}
