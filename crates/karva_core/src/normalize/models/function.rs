use std::collections::HashMap;

use pyo3::prelude::*;

use crate::{
    extensions::{fixtures::NormalizedFixture, tags::Tags},
    name::QualifiedFunctionName,
};

/// A normalized test function represents a concrete variant of a test after parametrization.
/// For parametrized tests, each parameter combination gets its own `NormalizedTestFunction`.
#[derive(Debug)]
pub struct NormalizedTestFunction {
    /// Original test function name: "`test_foo`"
    pub(crate) original_name: QualifiedFunctionName,

    /// Location in source code: "<`file_path>`:<`line_number`>"
    pub(crate) location: String,

    /// Test-level parameters (from @pytest.mark.parametrize)
    /// Maps parameter name to its value for this variant
    pub(crate) params: HashMap<String, Py<PyAny>>,

    /// Normalized fixture dependencies (already expanded)
    /// Each fixture dependency is a specific variant
    /// These are the regular fixtures that should be passed as arguments to the test function
    pub(crate) fixture_dependencies: Vec<NormalizedFixture>,

    /// Fixtures from `use_fixtures` tag that should only be executed for side effects
    /// These should NOT be passed as arguments to the test function
    pub(crate) use_fixture_dependencies: Vec<NormalizedFixture>,

    pub(crate) auto_use_fixtures: Vec<NormalizedFixture>,

    /// Original test metadata
    pub(crate) function: Py<PyAny>,
    pub(crate) tags: Tags,
}

impl NormalizedTestFunction {
    #[expect(clippy::too_many_arguments)]
    pub(crate) const fn new(
        original_name: QualifiedFunctionName,
        location: String,
        params: HashMap<String, Py<PyAny>>,
        fixture_dependencies: Vec<NormalizedFixture>,
        use_fixture_dependencies: Vec<NormalizedFixture>,
        auto_use_fixtures: Vec<NormalizedFixture>,
        function: Py<PyAny>,
        tags: Tags,
    ) -> Self {
        Self {
            original_name,
            location,
            params,
            fixture_dependencies,
            use_fixture_dependencies,
            auto_use_fixtures,
            function,
            tags,
        }
    }

    pub(crate) const fn original_name(&self) -> &QualifiedFunctionName {
        &self.original_name
    }

    pub(crate) fn location(&self) -> &str {
        &self.location
    }

    pub(crate) const fn params(&self) -> &HashMap<String, Py<PyAny>> {
        &self.params
    }

    pub(crate) fn fixture_dependencies(&self) -> &[NormalizedFixture] {
        &self.fixture_dependencies
    }

    pub(crate) fn use_fixture_dependencies(&self) -> &[NormalizedFixture] {
        &self.use_fixture_dependencies
    }

    pub(crate) const fn function(&self) -> &Py<PyAny> {
        &self.function
    }

    pub(crate) const fn tags(&self) -> &Tags {
        &self.tags
    }
}
