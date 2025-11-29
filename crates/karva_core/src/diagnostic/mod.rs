mod metadata;
mod reporter;
mod traceback;

use karva_project::path::TestPathError;
pub use metadata::{DiagnosticGuardBuilder, DiagnosticType};
pub use reporter::{DummyReporter, Reporter, TestCaseReporter};
use ruff_db::diagnostic::{Annotation, Diagnostic, Severity, Span};
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::{SourceFile, SourceFileBuilder};

use crate::{Context, declare_diagnostic_type, discovery::DiscoveredModule};

declare_diagnostic_type! {
    /// ## Invalid path
    ///
    /// User gave an invalid path
    pub(crate) static INVALID_PATH = {
        summary: "User provided an invalid path",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Failed to import module
    pub(crate) static FAILED_TO_IMPORT_MODULE = {
        summary: "Failed to import python module",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Invalid fixture
    pub(crate) static INVALID_FIXTURE = {
        summary: "Discovered an invalid fixture",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Invalid fixture finalizer
    pub(crate) static INVALID_FIXTURE_FINALIZER = {
        summary: "Tried to run an invalid fixture finalizer",
        severity: Severity::Warning,
    }
}

declare_diagnostic_type! {
    /// ## Failed Test
    pub(crate) static TEST_FAILURE = {
        summary: "Test raises exception when run",
        severity: Severity::Error,
    }
}

pub(crate) fn report_invalid_path(context: &Context, error: TestPathError) {
    let builder = context.report_diagnostic(&INVALID_PATH);

    builder.into_diagnostic(format!("Invalid path: {}", error));
}

pub fn report_failed_to_import_module(context: &Context, module_name: &str) {
    let builder = context.report_diagnostic(&FAILED_TO_IMPORT_MODULE);

    builder.into_diagnostic(format!("Failed to import python module `{module_name}`"));
}

pub fn report_invalid_fixture(
    context: &Context,
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
    reason: &str,
) {
    let builder = context.report_diagnostic(&INVALID_FIXTURE);

    let mut diagnostic = builder.into_diagnostic(format!(
        "Discovered an invalid fixture `{}`",
        stmt_function_def.name
    ));

    let primary_span = Span::from(source_file).with_range(stmt_function_def.name.range);

    diagnostic.annotate(Annotation::primary(primary_span.clone()));

    diagnostic.info(format!("Reason: {}", reason));
}

pub fn report_invalid_fixture_finalizer(
    context: &Context,
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
    reason: &str,
) {
    let builder = context.report_diagnostic(&INVALID_FIXTURE_FINALIZER);

    let mut diagnostic = builder.into_diagnostic(format!(
        "Discovered an invalid fixture finalizer `{}`",
        stmt_function_def.name
    ));

    let primary_span = Span::from(source_file).with_range(stmt_function_def.name.range);

    diagnostic.annotate(Annotation::primary(primary_span.clone()));

    diagnostic.info(format!("Reason: {}", reason));
}
