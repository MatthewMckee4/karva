use std::sync::Arc;

use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::{
    QualifiedFunctionName,
    discovery::DiscoveredModule,
    extensions::{fixtures::RequiresFixtures, tags::Tags},
};

/// Represents a single test function discovered from Python source code.
#[derive(Debug)]
pub struct TestFunction {
    /// The name of the test function.
    pub(crate) name: QualifiedFunctionName,

    /// The ast function statement.
    pub(crate) stmt_function_def: Arc<StmtFunctionDef>,

    /// The Python function object.
    pub(crate) py_function: Py<PyAny>,

    /// The tags associated with the test function.
    pub(crate) tags: Tags,
}

impl TestFunction {
    pub(crate) fn new(
        py: Python<'_>,
        module: &DiscoveredModule,
        stmt_function_def: Arc<StmtFunctionDef>,
        py_function: Py<PyAny>,
    ) -> Self {
        let name = QualifiedFunctionName::new(
            stmt_function_def.name.to_string(),
            module.module_path().clone(),
        );

        let tags = Tags::from_py_any(py, &py_function, Some(&stmt_function_def));

        Self {
            name,
            stmt_function_def,
            py_function,
            tags,
        }
    }
}

impl RequiresFixtures for TestFunction {
    fn required_fixtures(&self, py: Python<'_>) -> Vec<String> {
        let mut required_fixtures = self.stmt_function_def.required_fixtures(py);

        required_fixtures.extend(self.tags.required_fixtures_names());

        required_fixtures
    }
}
