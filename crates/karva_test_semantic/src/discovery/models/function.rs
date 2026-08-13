use std::rc::Rc;

use karva_python_semantic::QualifiedFunctionName;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use ruff_python_ast::{Parameters, StmtFunctionDef};
use ruff_source_file::SourceFile;
use ruff_text_size::TextRange;

use crate::discovery::DiscoveredModule;
use crate::discovery::models::definition::TestDefinition;
use crate::extensions::tags::Tags;

/// Represents a single executable test discovered from Python source code.
///
/// Contains all the information needed to execute a test, including the
/// test's qualified name, source definition, Python callable, and tags.
#[derive(Debug)]
pub struct DiscoveredTestFunction {
    /// Immutable source identity and test-kind-specific syntax.
    definition: Rc<TestDefinition>,

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
    pub(crate) fn new_function(
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
            tags.extend(
                &Tags::from_pytest_marks(py, &marks.unbind(), Some(&py_module.dict()))?
                    .unwrap_or_default(),
            );
        }

        Ok(Self {
            definition: Rc::new(TestDefinition::function(
                name,
                stmt_function_def,
                module.source_file(),
            )),
            py_function,
            tags,
            case_filter,
        })
    }

    pub(crate) fn new_doctest(
        py: Python<'_>,
        module: &DiscoveredModule,
        py_module: &Bound<'_, PyModule>,
        name: String,
        range: TextRange,
        py_function: Py<PyAny>,
    ) -> PyResult<Self> {
        let name = QualifiedFunctionName::new(name, module.module_path().clone());
        let tags = if let Ok(marks) = py_module.getattr("pytestmark") {
            Tags::from_pytest_marks(py, &marks.unbind(), Some(&py_module.dict()))?
                .unwrap_or_default()
        } else {
            Tags::default()
        };

        Ok(Self {
            definition: Rc::new(TestDefinition::doctest(name, range, module.source_file())),
            py_function,
            tags,
            case_filter: None,
        })
    }

    pub(crate) fn definition(&self) -> &Rc<TestDefinition> {
        &self.definition
    }

    pub(crate) fn name(&self) -> &QualifiedFunctionName {
        self.definition.name()
    }

    pub(super) fn source_range(&self) -> TextRange {
        self.definition.source_range()
    }

    pub(crate) fn diagnostic_range(&self) -> TextRange {
        self.definition.diagnostic_range()
    }

    pub(crate) fn function_statement(&self) -> Option<&StmtFunctionDef> {
        self.definition.function_statement()
    }

    pub(crate) fn parameters(&self) -> Option<&Parameters> {
        self.definition.parameters()
    }

    pub(crate) fn is_async(&self) -> bool {
        self.definition.is_async()
    }

    pub(crate) fn required_fixtures(&self) -> Vec<String> {
        self.definition.required_fixtures()
    }

    pub(crate) fn source_file(&self) -> &SourceFile {
        self.definition.source_file()
    }
}
