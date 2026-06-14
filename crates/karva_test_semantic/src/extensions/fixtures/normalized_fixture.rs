use std::collections::HashMap;
use std::rc::Rc;

use karva_python_semantic::QualifiedFunctionName;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

use crate::extensions::fixtures::FixtureScope;
use crate::extensions::tags::Tags;
use crate::utils::run_coroutine;

/// A normalized fixture represents a concrete instance of a fixture ready for execution.
///
/// All fixtures — both user-defined and framework-provided — share this single
/// representation. Framework fixtures (from `karva._builtins`) are discovered
/// and normalized the same way as user-defined ones.
#[derive(Debug, Clone)]
pub struct NormalizedFixture {
    /// Fully qualified name including module path and function name.
    pub(crate) name: QualifiedFunctionName,

    /// Resolved fixture dependencies this fixture requires.
    pub(crate) dependencies: Vec<Rc<Self>>,

    /// The scope at which this fixture's value is cached.
    pub(crate) scope: FixtureScope,

    /// Whether this fixture uses yield for teardown logic.
    pub(crate) is_generator: bool,

    /// Reference to the Python callable that produces the fixture value.
    /// Wrapped in ``Rc`` so ``NormalizedFixture`` stays cheaply ``Clone``
    /// without needing a Python token (``Py<T>`` only supports
    /// ``clone_ref(py)``).
    pub(crate) py_function: Rc<Py<PyAny>>,

    /// AST representation of the fixture function definition.
    pub(crate) stmt_function_def: Rc<StmtFunctionDef>,

    /// Source code captured during discovery for diagnostic reporting.
    pub(crate) source_file: SourceFile,
}

impl NormalizedFixture {
    /// Returns the fixture's unqualified function name.
    pub(crate) fn function_name(&self) -> &str {
        self.name.function_name()
    }

    /// Returns the fixture dependencies.
    pub(crate) fn dependencies(&self) -> &Vec<Rc<Self>> {
        &self.dependencies
    }

    /// Returns the fixture scope.
    pub(crate) fn scope(&self) -> FixtureScope {
        self.scope
    }

    pub(crate) fn resolved_tags(&self) -> Tags {
        let mut tags = Tags::default();

        for dependency in self.dependencies() {
            tags.extend(&dependency.resolved_tags());
        }

        tags
    }

    /// Call this fixture with the already-resolved arguments and return the result.
    pub(crate) fn call(
        &self,
        py: Python,
        fixture_arguments: &HashMap<String, Py<PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let result = if fixture_arguments.is_empty() {
            self.py_function.call0(py)
        } else {
            let kwargs_dict = PyDict::new(py);

            for (key, value) in fixture_arguments {
                kwargs_dict.set_item(key, value)?;
            }

            self.py_function.call(py, (), Some(&kwargs_dict))
        };

        if self.stmt_function_def.is_async && !self.is_generator {
            result.and_then(|coroutine| run_coroutine(py, coroutine))
        } else {
            result
        }
    }
}
