use std::rc::Rc;

use karva_python_semantic::QualifiedFunctionName;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

use crate::discovery::DiscoveredModule;
use crate::discovery::models::definition::FunctionDefinition;
use crate::extensions::tags::Tags;

/// Represents a single test function discovered from Python source code.
///
/// Contains all the information needed to execute a test, including the
/// function's qualified name, AST representation, Python callable, and
/// any associated decorator tags.
#[derive(Debug)]
pub struct DiscoveredTestFunction {
    /// Immutable source identity and syntax.
    definition: Rc<FunctionDefinition>,

    /// Reference to the actual Python callable object.
    pub(crate) py_function: Py<PyAny>,

    /// Decorator tags like parametrize, skip, xfail, etc.
    pub(crate) tags: Tags,

    /// Restrict execution to these parametrize case indices when `Some`,
    /// or run every case when `None`. Set by the worker CLI when the user
    /// (or partitioner) requested a subset like `file::test[3]`.
    pub(crate) case_filter: Option<Vec<usize>>,
}

impl DiscoveredTestFunction {
    pub(crate) fn new(
        py: Python<'_>,
        module: &DiscoveredModule,
        py_module: &Bound<'_, PyModule>,
        stmt_function_def: Rc<StmtFunctionDef>,
        py_function: Py<PyAny>,
        case_filter: Option<Vec<usize>>,
    ) -> PyResult<Self> {
        let name = QualifiedFunctionName::new(
            stmt_function_def.name.to_string(),
            module.module_path().clone(),
        );

        let mut tags = Tags::from_py_any(py, &py_function, Some(&stmt_function_def))?;
        if let Ok(marks) = py_module.getattr("pytestmark") {
            let module_tags =
                Tags::from_pytest_marks(py, &marks.unbind(), Some(&py_module.dict()))?
                    .unwrap_or_default();
            tags.extend(&module_tags);
        }

        Ok(Self {
            definition: Rc::new(FunctionDefinition::new(
                name,
                stmt_function_def,
                module.source_file(),
            )),
            py_function,
            tags,
            case_filter,
        })
    }

    pub(crate) fn definition(&self) -> &Rc<FunctionDefinition> {
        &self.definition
    }

    pub(crate) fn name(&self) -> &QualifiedFunctionName {
        self.definition.name()
    }

    pub(crate) fn statement(&self) -> &StmtFunctionDef {
        self.definition.statement()
    }

    pub(crate) fn source_file(&self) -> &SourceFile {
        self.definition.source_file()
    }
}
