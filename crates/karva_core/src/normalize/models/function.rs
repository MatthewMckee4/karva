use std::collections::HashMap;

use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::extensions::{fixtures::NormalizedFixture, tags::Tags};

/// A normalized test function represents a concrete variant of a test after parametrization.
/// For parametrized tests, each parameter combination gets its own `NormalizedTestFunction`.
#[derive(Debug)]
pub struct NormalizedTestFunction {
    /// Unique name including all parameters: "`test_foo`[x=1,y='a']"
    /// For non-parametrized tests, this is the same as `original_name`
    pub(crate) name: String,

    /// Original test function name: "`test_foo`"
    pub(crate) original_name: String,

    /// Qualified name with module path: "<module_path>::test_foo"
    pub(crate) qualified_name: String,

    /// Location in source code: "<file_path>:<line_number>"
    pub(crate) location: String,

    /// Test-level parameters (from @pytest.mark.parametrize)
    /// Maps parameter name to its value for this variant
    pub(crate) params: HashMap<String, Py<PyAny>>,

    /// Normalized fixture dependencies (already expanded)
    /// Each fixture dependency is a specific variant
    /// These are the regular fixtures that should be passed as arguments to the test function
    pub(crate) fixture_dependencies: Vec<NormalizedFixture>,

    /// Fixtures from use_fixtures tag that should only be executed for side effects
    /// These should NOT be passed as arguments to the test function
    pub(crate) use_fixture_dependencies: Vec<NormalizedFixture>,

    /// Original test metadata
    pub(crate) function: Py<PyAny>,
    pub(crate) function_definition: StmtFunctionDef,
    pub(crate) tags: Tags,
}

impl NormalizedTestFunction {
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        name: String,
        original_name: String,
        qualified_name: String,
        location: String,
        params: HashMap<String, Py<PyAny>>,
        fixture_dependencies: Vec<NormalizedFixture>,
        use_fixture_dependencies: Vec<NormalizedFixture>,
        function: Py<PyAny>,
        function_definition: StmtFunctionDef,
        tags: Tags,
    ) -> Self {
        Self {
            name,
            original_name,
            qualified_name,
            location,
            params,
            fixture_dependencies,
            use_fixture_dependencies,
            function,
            function_definition,
            tags,
        }
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn original_name(&self) -> &str {
        &self.original_name
    }

    pub(crate) fn qualified_name(&self) -> &str {
        &self.qualified_name
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

    pub(crate) const fn function_definition(&self) -> &StmtFunctionDef {
        &self.function_definition
    }

    pub(crate) const fn tags(&self) -> &Tags {
        &self.tags
    }
}
