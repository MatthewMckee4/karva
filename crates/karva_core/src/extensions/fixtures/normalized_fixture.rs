use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::{extensions::fixtures::FixtureScope, name::QualifiedFunctionName};

#[derive(Debug, Clone)]
pub(crate) enum NormalizedFixtureName {
    BuiltIn(String),
    UserDefined(QualifiedFunctionName),
}

impl NormalizedFixtureName {
    pub(crate) fn function_name(&self) -> &str {
        match self {
            Self::BuiltIn(name) => name,
            Self::UserDefined(name) => name.function_name(),
        }
    }
}

impl std::fmt::Display for NormalizedFixtureName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BuiltIn(name) => write!(f, "{name}"),
            Self::UserDefined(name) => write!(f, "{name}"),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum NormalizedFixtureValue {
    /// For now, just used for builtin fixtures where we compute the value early
    Computed(Py<PyAny>),
    /// Normal fixtures just have a function that needs to be called to compute the value
    Function(Py<PyAny>),
}

/// A normalized fixture represents a concrete variant of a fixture after parametrization.
/// For parametrized fixtures, each parameter value gets its own `NormalizedFixture`.
#[derive(Debug, Clone)]
pub(crate) struct NormalizedFixture {
    /// Original fixture name without parameter: "`my_fixture`"
    pub(crate) name: NormalizedFixtureName,

    /// The specific parameter value for this variant (if parametrized)
    pub(crate) param: Option<Py<PyAny>>,

    /// Normalized dependencies (already expanded for their params)
    /// Each dependency is already a specific variant
    pub(crate) dependencies: Vec<NormalizedFixture>,

    /// Location in source code: "<`file_path>`:<`line_number`>"
    /// None for builtin fixtures
    pub(crate) location: String,

    /// Original fixture metadata
    pub(crate) scope: FixtureScope,

    pub(crate) is_generator: bool,

    pub(crate) value: NormalizedFixtureValue,

    pub(crate) function_definition: Option<StmtFunctionDef>,
}

impl NormalizedFixture {
    #[allow(clippy::too_many_arguments)]
    pub(crate) const fn new(
        name: NormalizedFixtureName,
        param: Option<Py<PyAny>>,
        dependencies: Vec<Self>,
        location: String,
        scope: FixtureScope,
        is_generator: bool,
        value: NormalizedFixtureValue,
        function_definition: Option<StmtFunctionDef>,
    ) -> Self {
        Self {
            name,
            param,
            dependencies,
            location,
            scope,
            is_generator,
            value,
            function_definition,
        }
    }

    pub(crate) const fn name(&self) -> &NormalizedFixtureName {
        &self.name
    }

    pub(crate) const fn param(&self) -> Option<&Py<PyAny>> {
        self.param.as_ref()
    }

    pub(crate) fn dependencies(&self) -> &[Self] {
        &self.dependencies
    }

    pub(crate) const fn function_definition(&self) -> Option<&StmtFunctionDef> {
        self.function_definition.as_ref()
    }

    /// Creates a built-in fixture (like `tmp_path`) that doesn't have a Python definition.
    /// These fixtures are provided by the framework itself.
    pub(crate) const fn built_in(_py: Python<'_>, name: String, value: Py<PyAny>) -> Self {
        Self::new(
            NormalizedFixtureName::BuiltIn(name),
            None,
            vec![],
            String::new(),
            FixtureScope::Function,
            false,
            NormalizedFixtureValue::Computed(value),
            None,
        )
    }
}
