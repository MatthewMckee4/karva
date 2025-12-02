use std::sync::Arc;

use camino::Utf8PathBuf;
use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::{
    QualifiedFunctionName,
    extensions::{
        fixtures::FixtureScope,
        tags::{Parametrization, Tags},
    },
};

#[derive(Debug, Clone)]
pub enum NormalizedFixtureValue {
    /// For now, just used for builtin fixtures where we compute the value early
    Computed(Py<PyAny>),
    /// Normal fixtures just have a function that needs to be called to compute the value
    Function(Py<PyAny>),
}

/// Built-in fixture data
#[derive(Debug, Clone)]
pub struct BuiltInFixture {
    /// Built-in fixture name
    pub(crate) name: String,
    /// Pre-computed value for the built-in fixture
    pub(crate) value: NormalizedFixtureValue,
    /// Normalized dependencies (already expanded for their params)
    pub(crate) dependencies: Arc<Vec<NormalizedFixture>>,
    /// Fixture scope
    pub(crate) scope: FixtureScope,
    /// Optional finalizer to call after the fixture is used
    pub(crate) finalizer: Option<Py<PyAny>>,
}

/// User-defined fixture data
#[derive(Debug, Clone)]
pub struct UserDefinedFixture {
    /// Qualified function name
    pub(crate) name: QualifiedFunctionName,
    /// The specific parameter value for this variant (if parametrized)
    pub(crate) param: Option<Parametrization>,
    /// Normalized dependencies (already expanded for their params)
    pub(crate) dependencies: Arc<Vec<NormalizedFixture>>,
    /// Fixture scope
    pub(crate) scope: FixtureScope,
    /// If this fixture is a generator
    pub(crate) is_generator: bool,
    /// The computed value or imported python function to compute the value
    pub(crate) value: NormalizedFixtureValue,
    /// The function definition for this fixture
    pub(crate) stmt_function_def: Arc<StmtFunctionDef>,
}

impl UserDefinedFixture {
    pub(crate) const fn module_path(&self) -> &Utf8PathBuf {
        self.name.module_path().path()
    }
}

/// A normalized fixture represents a concrete variant of a fixture after parametrization.
/// For parametrized fixtures, each parameter value gets its own `NormalizedFixture`.
///
/// We choose to make all variables `pub(crate)` so we can destructure and consume when needed.
#[derive(Debug, Clone)]
pub enum NormalizedFixture {
    BuiltIn(BuiltInFixture),
    UserDefined(UserDefinedFixture),
}

impl NormalizedFixture {
    /// Creates a built-in fixture that doesn't have a Python definition.
    pub(crate) fn built_in(name: String, value: Py<PyAny>) -> Self {
        Self::BuiltIn(BuiltInFixture {
            name,
            value: NormalizedFixtureValue::Computed(value),
            dependencies: Arc::new(vec![]),
            scope: FixtureScope::Function,
            finalizer: None,
        })
    }

    /// Creates a built-in fixture with a finalizer.
    pub(crate) fn built_in_with_finalizer(
        name: String,
        value: Py<PyAny>,
        finalizer: Py<PyAny>,
    ) -> Self {
        Self::BuiltIn(BuiltInFixture {
            name,
            value: NormalizedFixtureValue::Computed(value),
            dependencies: Arc::new(vec![]),
            scope: FixtureScope::Function,
            finalizer: Some(finalizer),
        })
    }

    /// Returns the fixture name (as `NormalizedFixtureName`)
    pub(crate) fn function_name(&self) -> &str {
        match self {
            Self::BuiltIn(fixture) => fixture.name.as_str(),
            Self::UserDefined(fixture) => fixture.name.function_name(),
        }
    }

    /// Returns the parameter value if this is a parametrized fixture
    pub(crate) const fn param(&self) -> Option<&Parametrization> {
        match self {
            Self::BuiltIn(_) => None,
            Self::UserDefined(fixture) => fixture.param.as_ref(),
        }
    }

    /// Returns the fixture dependencies
    pub(crate) fn dependencies(&self) -> &[Self] {
        match self {
            Self::BuiltIn(fixture) => &fixture.dependencies,
            Self::UserDefined(fixture) => &fixture.dependencies,
        }
    }

    /// Returns the fixture scope
    pub(crate) const fn scope(&self) -> FixtureScope {
        match self {
            Self::BuiltIn(fixture) => fixture.scope,
            Self::UserDefined(fixture) => fixture.scope,
        }
    }

    /// Returns whether this fixture is a generator (always false for built-in)
    pub(crate) const fn is_generator(&self) -> bool {
        match self {
            Self::BuiltIn(_) => false,
            Self::UserDefined(fixture) => fixture.is_generator,
        }
    }

    /// Returns the fixture value
    pub(crate) const fn value(&self) -> &NormalizedFixtureValue {
        match self {
            Self::BuiltIn(fixture) => &fixture.value,
            Self::UserDefined(fixture) => &fixture.value,
        }
    }

    /// Returns the function definition (None for built-in fixtures)
    pub(crate) const fn stmt_function_def(&self) -> Option<&Arc<StmtFunctionDef>> {
        match self {
            Self::BuiltIn(_) => None,
            Self::UserDefined(fixture) => Some(&fixture.stmt_function_def),
        }
    }

    pub(crate) const fn as_user_defined(&self) -> Option<&UserDefinedFixture> {
        if let Self::UserDefined(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub(crate) const fn as_builtin(&self) -> Option<&BuiltInFixture> {
        if let Self::BuiltIn(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub(crate) fn resolved_tags(&self) -> Tags {
        let mut tags = self
            .param()
            .map(|param| param.tags().clone())
            .unwrap_or_default();

        for dependency in self.dependencies() {
            tags.extend(&dependency.resolved_tags());
        }

        tags
    }
}
