use std::rc::Rc;

use karva_python_semantic::QualifiedFunctionName;
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

/// Immutable source identity shared by discovered tests, fixtures, and diagnostics.
#[derive(Debug)]
pub struct FunctionDefinition {
    name: QualifiedFunctionName,
    statement: Rc<StmtFunctionDef>,
    source_file: SourceFile,
}

impl FunctionDefinition {
    pub(crate) fn new(
        name: QualifiedFunctionName,
        statement: Rc<StmtFunctionDef>,
        source_file: SourceFile,
    ) -> Self {
        Self {
            name,
            statement,
            source_file,
        }
    }

    pub(crate) fn name(&self) -> &QualifiedFunctionName {
        &self.name
    }

    pub(crate) fn statement(&self) -> &StmtFunctionDef {
        &self.statement
    }

    pub(crate) fn statement_rc(&self) -> &Rc<StmtFunctionDef> {
        &self.statement
    }

    pub(crate) fn source_file(&self) -> &SourceFile {
        &self.source_file
    }
}
