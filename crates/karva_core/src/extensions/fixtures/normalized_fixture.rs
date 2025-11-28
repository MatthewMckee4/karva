use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::{Location, QualifiedFunctionName, extensions::fixtures::FixtureScope};

#[derive(Debug, Clone)]
pub enum NormalizedFixtureName {
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
pub enum NormalizedFixtureValue {
    /// For now, just used for builtin fixtures where we compute the value early
    Computed(Py<PyAny>),
    /// Normal fixtures just have a function that needs to be called to compute the value
    Function(Py<PyAny>),
}

/// A normalized fixture represents a concrete variant of a fixture after parametrization.
/// For parametrized fixtures, each parameter value gets its own `NormalizedFixture`.
#[derive(Debug, Clone)]
pub struct NormalizedFixture {
    /// Original fixture name
    pub(crate) name: NormalizedFixtureName,

    /// The specific parameter value for this variant (if parametrized)
    pub(crate) param: Option<Py<PyAny>>,

    /// Normalized dependencies (already expanded for their params)
    /// Each dependency is already a specific variant
    pub(crate) dependencies: Vec<NormalizedFixture>,

    /// Location in source code: "<`file_path>`:<`line_number`>"
    /// None for builtin fixtures
    pub(crate) location: Option<Location>,

    /// Original fixture metadata
    pub(crate) scope: FixtureScope,

    /// If this fixture is a generator
    pub(crate) is_generator: bool,

    /// The computed value or imported python function to compute the value
    pub(crate) value: NormalizedFixtureValue,

    /// The function definition for this fixture
    /// None for builtin fixtures
    pub(crate) function_definition: Option<StmtFunctionDef>,
}

impl NormalizedFixture {
    /// Creates a built-in fixture that doesn't have a Python definition.
    pub(crate) const fn built_in(name: String, value: Py<PyAny>) -> Self {
        Self {
            name: NormalizedFixtureName::BuiltIn(name),
            param: None,
            dependencies: vec![],
            location: None,
            scope: FixtureScope::Function,
            is_generator: false,
            value: NormalizedFixtureValue::Computed(value),
            function_definition: None,
        }
    }
}
