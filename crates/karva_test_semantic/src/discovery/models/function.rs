use std::rc::Rc;

use karva_python_semantic::QualifiedFunctionName;
use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

use crate::discovery::DiscoveredModule;
use crate::extensions::tags::Tags;

/// Represents a single test function discovered from Python source code.
///
/// Contains all the information needed to execute a test, including the
/// function's qualified name, AST representation, Python callable, and
/// any associated decorator tags.
#[derive(Debug)]
pub struct DiscoveredTestFunction {
    /// Fully qualified name including module path and function name.
    pub(crate) name: QualifiedFunctionName,

    /// AST representation of the function definition.
    pub(crate) stmt_function_def: Rc<StmtFunctionDef>,

    /// Source code captured during discovery for diagnostic reporting.
    pub(crate) source_file: SourceFile,

    /// Reference to the actual Python callable object.
    pub(crate) py_function: Py<PyAny>,

    /// Decorator tags like parametrize, skip, xfail, etc.
    pub(crate) tags: Tags,
}

impl DiscoveredTestFunction {
    pub(crate) fn new(
        py: Python<'_>,
        module: &DiscoveredModule,
        stmt_function_def: Rc<StmtFunctionDef>,
        py_function: Py<PyAny>,
    ) -> PyResult<Self> {
        let name = QualifiedFunctionName::new(
            stmt_function_def.name.to_string(),
            module.module_path().clone(),
        );

        let tags = Tags::from_py_any(py, &py_function, Some(&stmt_function_def))?;

        Ok(Self {
            name,
            stmt_function_def,
            source_file: module.source_file(),
            py_function,
            tags,
        })
    }
}
