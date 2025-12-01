use pyo3::prelude::*;

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
    pub(crate) const fn new(fixture_names: Vec<String>) -> Self {
        Self { fixture_names }
    }

    pub(crate) fn fixture_names(&self) -> &[String] {
        &self.fixture_names
    }

    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let args = py_mark.getattr("args").ok()?;
        args.extract::<Vec<String>>()
            .map_or(None, |fixture_names| Some(Self::new(fixture_names)))
    }
}
