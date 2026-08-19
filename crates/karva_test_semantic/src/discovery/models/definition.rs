use std::rc::Rc;

use karva_python_semantic::QualifiedFunctionName;
use ruff_python_ast::{Parameters, StmtFunctionDef};
use ruff_source_file::SourceFile;
use ruff_text_size::TextRange;

/// Returns names Python requires and Karva can supply by keyword.
pub fn required_keyword_parameter_names(parameters: &Parameters) -> Vec<String> {
    parameters
        .args
        .iter()
        .chain(&parameters.kwonlyargs)
        .filter(|parameter| parameter.default().is_none())
        .map(|parameter| parameter.name().to_string())
        .collect()
}

/// Immutable source identity shared by fixtures and fixture diagnostics.
#[derive(Debug)]
pub struct FunctionDefinition {
    name: QualifiedFunctionName,
    /// Unqualified name shared by fixture execution argument maps.
    argument_name: Rc<str>,
    statement: Rc<StmtFunctionDef>,
    source_file: SourceFile,
}

impl FunctionDefinition {
    pub(crate) fn new(
        name: QualifiedFunctionName,
        statement: Rc<StmtFunctionDef>,
        source_file: SourceFile,
    ) -> Self {
        let argument_name = Rc::from(name.function_name());
        Self {
            name,
            argument_name,
            statement,
            source_file,
        }
    }

    pub(crate) fn name(&self) -> &QualifiedFunctionName {
        &self.name
    }

    /// Returns the fixture name shared by call argument maps.
    pub(crate) fn argument_name(&self) -> &Rc<str> {
        &self.argument_name
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

/// Immutable source identity for an executable test.
///
/// Python functions retain their source AST. Doctests retain only real source metadata because
/// they have no corresponding function definition.
#[derive(Debug)]
pub struct TestDefinition {
    name: QualifiedFunctionName,
    source_file: SourceFile,
    kind: TestDefinitionKind,
}

#[derive(Debug)]
enum TestDefinitionKind {
    /// A Python test function backed by its parsed source definition.
    Function(Rc<StmtFunctionDef>),

    /// A doctest located at its first executable prompt.
    Doctest { range: TextRange },
}

impl TestDefinition {
    pub(super) fn function(
        name: QualifiedFunctionName,
        statement: Rc<StmtFunctionDef>,
        source_file: SourceFile,
    ) -> Self {
        Self {
            name,
            source_file,
            kind: TestDefinitionKind::Function(statement),
        }
    }

    pub(super) fn doctest(
        name: QualifiedFunctionName,
        range: TextRange,
        source_file: SourceFile,
    ) -> Self {
        Self {
            name,
            source_file,
            kind: TestDefinitionKind::Doctest { range },
        }
    }

    pub(crate) fn name(&self) -> &QualifiedFunctionName {
        &self.name
    }

    pub(crate) fn source_file(&self) -> &SourceFile {
        &self.source_file
    }

    pub(super) fn source_range(&self) -> TextRange {
        match &self.kind {
            TestDefinitionKind::Function(statement) => statement.range,
            TestDefinitionKind::Doctest { range } => *range,
        }
    }

    /// Returns the source range to underline in diagnostics.
    pub(crate) fn diagnostic_range(&self) -> TextRange {
        match &self.kind {
            TestDefinitionKind::Function(statement) => statement.name.range,
            TestDefinitionKind::Doctest { range } => *range,
        }
    }

    /// Returns syntax available only for regular Python test functions.
    pub(super) fn function_statement(&self) -> Option<&StmtFunctionDef> {
        match &self.kind {
            TestDefinitionKind::Function(statement) => Some(statement),
            TestDefinitionKind::Doctest { .. } => None,
        }
    }

    pub(crate) fn parameters(&self) -> Option<&Parameters> {
        self.function_statement()
            .map(|statement| statement.parameters.as_ref())
    }

    pub(super) fn is_async(&self) -> bool {
        self.function_statement()
            .is_some_and(|statement| statement.is_async)
    }

    pub(super) fn required_fixtures(&self) -> Vec<String> {
        self.parameters()
            .map_or_else(Vec::new, required_keyword_parameter_names)
    }
}
