use std::collections::HashMap;
use std::rc::Rc;

use karva_python_semantic::QualifiedFunctionName;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use ruff_python_ast::StmtFunctionDef;

use crate::extensions::fixtures::FixtureScope;
use crate::extensions::tags::Tags;

/// Represents a built-in fixture provided by the test framework.
///
/// Built-in fixtures like `request`, `tmp_path`, or `monkeypatch` are
/// provided by the framework rather than defined in user code.
#[derive(Debug, Clone)]
pub struct BuiltInFixture {
    /// Name of the built-in fixture.
    pub(crate) name: String,

    /// Pre-computed Python value for this fixture.
    pub(crate) py_value: Rc<Py<PyAny>>,

    /// Resolved fixture dependencies.
    pub(crate) dependencies: Vec<Rc<NormalizedFixture>>,

    /// The scope at which this fixture's value is cached.
    pub(crate) scope: FixtureScope,

    /// Optional cleanup function to call after the fixture is used.
    pub(crate) finalizer: Option<Rc<Py<PyAny>>>,
}

/// Represents a user-defined fixture from Python test code.
///
/// User-defined fixtures are created with the `@fixture` decorator and
/// provide custom setup/teardown logic for tests.
#[derive(Debug, Clone)]
pub struct UserDefinedFixture {
    /// Fully qualified name including module path and function name.
    pub(crate) name: QualifiedFunctionName,

    /// Resolved fixture dependencies this fixture requires.
    pub(crate) dependencies: Vec<Rc<NormalizedFixture>>,

    /// The scope at which this fixture's value is cached.
    pub(crate) scope: FixtureScope,

    /// Whether this fixture uses yield for teardown logic.
    pub(crate) is_generator: bool,

    /// Reference to the Python callable that produces the fixture value.
    pub(crate) py_function: Rc<Py<PyAny>>,

    /// AST representation of the fixture function definition.
    pub(crate) stmt_function_def: Rc<StmtFunctionDef>,
}

/// A normalized fixture represents a concrete instance of a fixture.
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
            py_value: Rc::new(value),
            dependencies: vec![],
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
            py_value: Rc::new(value),
            dependencies: vec![],
            scope: FixtureScope::Function,
            finalizer: Some(Rc::new(finalizer)),
        })
    }

    /// Returns the fixture name (as `NormalizedFixtureName`)
    pub(crate) fn function_name(&self) -> &str {
        match self {
            Self::BuiltIn(fixture) => fixture.name.as_str(),
            Self::UserDefined(fixture) => fixture.name.function_name(),
        }
    }

    /// Returns the fixture dependencies
    pub(crate) fn dependencies(&self) -> &Vec<Rc<Self>> {
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
        let mut tags = Tags::default();

        for dependency in self.dependencies() {
            tags.extend(&dependency.resolved_tags());
        }

        tags
    }

    /// Call this fixture with the already resolved arguments and return the result.
    pub(crate) fn call(
        &self,
        py: Python,
        fixture_arguments: &HashMap<String, Py<PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        // For builtin fixtures, the value is stored directly in the function field
        // and function_definition is None. Return the value directly without calling.
        match self {
            Self::BuiltIn(built_in_fixture) => Ok(built_in_fixture.py_value.clone_ref(py)),
            Self::UserDefined(user_defined_fixture) => {
                let kwargs_dict = PyDict::new(py);

                for (key, value) in fixture_arguments {
                    let _ = kwargs_dict.set_item(key.clone(), value);
                }

                if kwargs_dict.is_empty() {
                    user_defined_fixture.py_function.call0(py)
                } else {
                    user_defined_fixture
                        .py_function
                        .call(py, (), Some(&kwargs_dict))
                }
            }
        }
    }

    /// Returns `true` if the normalized fixture is [`UserDefined`].
    ///
    /// [`UserDefined`]: NormalizedFixture::UserDefined
    #[must_use]
    pub fn is_user_defined(&self) -> bool {
        matches!(self, Self::UserDefined(..))
    }
}
