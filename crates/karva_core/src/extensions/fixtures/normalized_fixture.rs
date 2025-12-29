use std::collections::HashMap;
use std::sync::Arc;

use karva_python_semantic::QualifiedFunctionName;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use ruff_python_ast::StmtFunctionDef;

use crate::extensions::fixtures::{
    FixtureRequest, FixtureScope, RequiresFixtures, python::TestNode,
};
use crate::extensions::tags::{Parametrization, Tags};

/// Built-in fixture data
#[derive(Debug, Clone)]
pub struct BuiltInFixture {
    /// Built-in fixture name
    pub(crate) name: String,
    /// Pre-computed value for the built-in fixture
    pub(crate) py_value: Py<PyAny>,
    /// Normalized dependencies (already expanded for their params)
    pub(crate) dependencies: Vec<Arc<NormalizedFixture>>,
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
    pub(crate) dependencies: Vec<Arc<NormalizedFixture>>,
    /// Fixture scope
    pub(crate) scope: FixtureScope,
    /// If this fixture is a generator
    pub(crate) is_generator: bool,
    /// The computed value or imported python function to compute the value
    pub(crate) py_function: Py<PyAny>,
    /// The function definition for this fixture
    pub(crate) stmt_function_def: Arc<StmtFunctionDef>,
    /// Test name for the current test context (when executing within a test)
    pub(crate) test_name: Option<String>,
    /// Test tags for the current test context (when executing within a test)
    pub(crate) test_tags: Option<crate::extensions::tags::python::PyTags>,
    /// Pytest marks for the current test context (when executing within a test)
    pub(crate) pytest_marks: Option<Py<PyAny>>,
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
            py_value: value,
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
            py_value: value,
            dependencies: vec![],
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
    pub(crate) fn dependencies(&self) -> &Vec<Arc<Self>> {
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
        let mut tags = self
            .param()
            .map(|param| param.tags().clone())
            .unwrap_or_default();

        for dependency in self.dependencies() {
            tags.extend(&dependency.resolved_tags());
        }

        tags
    }

    /// Create a version of this fixture with test tags populated.
    /// This is used when executing a fixture in a test context.
    pub(crate) fn with_test_context(
        &self,
        test_name: Option<String>,
        test_tags: Option<crate::extensions::tags::python::PyTags>,
        pytest_marks: Option<Py<PyAny>>,
    ) -> Self {
        match self {
            Self::BuiltIn(b) => Self::BuiltIn(b.clone()),
            Self::UserDefined(u) => {
                let mut updated = u.clone();
                updated.test_name = test_name;
                updated.test_tags = test_tags;
                updated.pytest_marks = pytest_marks;
                Self::UserDefined(updated)
            }
        }
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
            Self::BuiltIn(built_in_fixture) => Ok(built_in_fixture.py_value.clone()),
            Self::UserDefined(user_defined_fixture) => {
                let kwargs_dict = PyDict::new(py);

                for (key, value) in fixture_arguments {
                    let _ = kwargs_dict.set_item(key.clone(), value);
                }

                let required = user_defined_fixture.stmt_function_def.required_fixtures(py);

                if required.contains(&"request".to_string()) {
                    let param_value = user_defined_fixture
                        .param
                        .as_ref()
                        .and_then(|param| param.values.first())
                        .cloned();

                    // Always create a TestNode (even if empty) for pytest compatibility
                    let tags_clone = user_defined_fixture.test_tags.clone().unwrap_or_else(|| {
                        crate::extensions::tags::python::PyTags { inner: vec![] }
                    });
                    let marks_clone = user_defined_fixture.pytest_marks.clone();
                    let test_name_clone = user_defined_fixture.test_name.clone();
                    let node = Some(TestNode::new(test_name_clone, tags_clone, marks_clone));

                    if let Ok(request_obj) =
                        FixtureRequest::new(py, param_value, node).and_then(|req| Py::new(py, req))
                    {
                        kwargs_dict.set_item("request", request_obj).ok();
                    }
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
}
