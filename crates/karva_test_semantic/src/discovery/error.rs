use std::rc::Rc;

use camino::Utf8PathBuf;
use karva_collector::CollectionError;
use karva_python_semantic::ModulePath;
use ruff_db::diagnostic::Diagnostic;
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;
use ruff_text_size::TextRange;

use crate::diagnostic::{
    collection_error_diagnostic, duplicate_fixture_diagnostic, duplicate_test_diagnostic,
    failed_to_discover_imported_fixture_diagnostic, failed_to_import_module_diagnostic,
    generator_test_diagnostic, invalid_fixture_diagnostic,
};

/// A failure found while binding collected Python definitions to runtime objects.
pub enum DiscoveryError {
    Collection(CollectionError),
    Import {
        module_name: String,
        reason: String,
    },
    ImportedFixture {
        fixture_name: String,
        source_path: Utf8PathBuf,
        error: std::io::Error,
    },
    DuplicateFixture {
        source_file: SourceFile,
        fixture_name: String,
        first_definition: Rc<StmtFunctionDef>,
        duplicate_definition: Rc<StmtFunctionDef>,
    },
    DuplicateTest {
        source_file: SourceFile,
        test_name: String,
        first_definition: Rc<StmtFunctionDef>,
        duplicate_definition: Rc<StmtFunctionDef>,
    },
    InvalidFixture {
        source_file: SourceFile,
        definition: Rc<StmtFunctionDef>,
        reason: String,
    },
    GeneratorTest {
        source_file: SourceFile,
        definition: Rc<StmtFunctionDef>,
    },
    UnknownTag {
        source_file: SourceFile,
        name: String,
        range: TextRange,
        suggestion: Option<String>,
    },
}

impl DiscoveryError {
    pub(crate) fn into_diagnostic(self) -> Diagnostic {
        match self {
            Self::Collection(error) => collection_error_diagnostic(&error),
            Self::Import {
                module_name,
                reason,
            } => failed_to_import_module_diagnostic(&module_name, &reason),
            Self::ImportedFixture {
                fixture_name,
                source_path,
                error,
            } => {
                failed_to_discover_imported_fixture_diagnostic(&fixture_name, &source_path, &error)
            }
            Self::DuplicateFixture {
                source_file,
                fixture_name,
                first_definition,
                duplicate_definition,
            } => duplicate_fixture_diagnostic(
                source_file,
                &fixture_name,
                &first_definition,
                &duplicate_definition,
            ),
            Self::DuplicateTest {
                source_file,
                test_name,
                first_definition,
                duplicate_definition,
            } => duplicate_test_diagnostic(
                source_file,
                &test_name,
                &first_definition,
                &duplicate_definition,
            ),
            Self::InvalidFixture {
                source_file,
                definition,
                reason,
            } => invalid_fixture_diagnostic(source_file, &definition, &reason),
            Self::GeneratorTest {
                source_file,
                definition,
            } => generator_test_diagnostic(source_file, &definition),
            Self::UnknownTag {
                source_file,
                name,
                range,
                suggestion,
            } => crate::diagnostic::unknown_tag_diagnostic(
                source_file,
                &name,
                range,
                suggestion.as_deref(),
            ),
        }
    }
}

/// Ordered discovery side effect, applied after discovery completes.
pub enum DiscoveryIssue {
    Error(DiscoveryError),
    SkippedModule {
        module_path: ModulePath,
        reason: Option<String>,
    },
}

/// Fully discovered test tree and the ordered issues produced while building it.
pub struct DiscoveryOutput {
    pub(crate) package: super::DiscoveredPackage,
    pub(crate) issues: Vec<DiscoveryIssue>,
}
