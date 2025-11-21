use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::{extensions::fixtures::FixtureScope, name::QualifiedFunctionName};

/// A normalized fixture represents a concrete variant of a fixture after parametrization.
/// For parametrized fixtures, each parameter value gets its own `NormalizedFixture`.
#[derive(Debug, Clone)]
pub(crate) struct NormalizedFixture {
    /// Unique name including parameter: "`my_fixture`[param1]"
    /// For non-parametrized fixtures, this is the same as `original_name`
    pub(crate) name: String,

    /// Original fixture name without parameter: "`my_fixture`"
    pub(crate) original_name: Option<QualifiedFunctionName>,

    /// The specific parameter value for this variant (if parametrized)
    pub(crate) param: Option<Py<PyAny>>,

    /// Normalized dependencies (already expanded for their params)
    /// Each dependency is already a specific variant
    pub(crate) dependencies: Vec<NormalizedFixture>,

    /// Location in source code: "<file_path>:<line_number>"
    /// None for builtin fixtures
    pub(crate) location: Option<String>,

    /// Original fixture metadata
    pub(crate) scope: FixtureScope,
    pub(crate) auto_use: bool,
    pub(crate) is_generator: bool,
    pub(crate) function: Py<PyAny>,
    pub(crate) function_definition: Option<StmtFunctionDef>,
}

impl NormalizedFixture {
    /// Creates a new normalized fixture
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        name: String,
        original_name: Option<QualifiedFunctionName>,
        param: Option<Py<PyAny>>,
        dependencies: Vec<Self>,
        location: Option<String>,
        scope: FixtureScope,
        auto_use: bool,
        is_generator: bool,
        function: Py<PyAny>,
        function_definition: Option<StmtFunctionDef>,
    ) -> Self {
        Self {
            name,
            original_name,
            param,
            dependencies,
            location,
            scope,
            auto_use,
            is_generator,
            function,
            function_definition,
        }
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn original_name(&self) -> &Option<QualifiedFunctionName> {
        &self.original_name
    }

    pub(crate) const fn param(&self) -> Option<&Py<PyAny>> {
        self.param.as_ref()
    }

    pub(crate) fn dependencies(&self) -> &[Self] {
        &self.dependencies
    }

    pub(crate) fn location(&self) -> Option<&str> {
        self.location.as_deref()
    }

    pub(crate) const fn scope(&self) -> FixtureScope {
        self.scope
    }

    pub(crate) const fn auto_use(&self) -> bool {
        self.auto_use
    }

    pub(crate) const fn is_generator(&self) -> bool {
        self.is_generator
    }

    pub(crate) const fn function(&self) -> &Py<PyAny> {
        &self.function
    }

    pub(crate) const fn function_definition(&self) -> &Option<StmtFunctionDef> {
        &self.function_definition
    }

    /// Creates a built-in fixture (like `tmp_path`) that doesn't have a Python definition.
    /// These fixtures are provided by the framework itself.
    pub(crate) fn built_in(_py: Python<'_>, name: String, value: Py<PyAny>) -> Self {
        Self::new(
            name.clone(),
            None,
            None,
            vec![],
            None, // No location for builtin fixtures
            FixtureScope::Function,
            false,
            false,
            value,
            None,
        )
    }
}
