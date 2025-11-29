mod metadata;
mod reporter;
mod result;
mod traceback;

use karva_project::path::TestPathError;
pub use metadata::{DiagnosticGuardBuilder, DiagnosticType};
use pyo3::{PyErr, Python};
pub use reporter::{DummyReporter, Reporter, TestCaseReporter};
pub use result::{IndividualTestResultKind, TestResultStats, TestRunResult};
use ruff_db::diagnostic::{
    Annotation, Diagnostic, Severity, Span, SubDiagnostic, SubDiagnosticSeverity,
};
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

use crate::{Context, declare_diagnostic_type, diagnostic::traceback::Traceback};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    Test,
    Fixture,
}

impl std::fmt::Display for FunctionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Test => write!(f, "test"),
            Self::Fixture => write!(f, "fixture"),
        }
    }
}

declare_diagnostic_type! {
    /// ## Invalid path
    ///
    /// User gave an invalid path
    pub static INVALID_PATH = {
        summary: "User provided an invalid path",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Failed to import module
    pub static FAILED_TO_IMPORT_MODULE = {
        summary: "Failed to import python module",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Invalid fixture
    pub static INVALID_FIXTURE = {
        summary: "Discovered an invalid fixture",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Invalid fixture finalizer
    pub static INVALID_FIXTURE_FINALIZER = {
        summary: "Tried to run an invalid fixture finalizer",
        severity: Severity::Warning,
    }
}

declare_diagnostic_type! {
    /// ## Missing fixtures
    pub static MISSING_FIXTURES = {
        summary: "Missing fixtures",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Failed Fixture
    pub static FIXTURE_FAILURE = {
        summary: "Fixture raises exception when run",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Test Passes when expected to fail
    pub static TEST_PASS_ON_EXPECT_FAILURE = {
        summary: "Test passes when expected to fail",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Failed Test
    pub static TEST_FAILURE = {
        summary: "Test raises exception when run",
        severity: Severity::Error,
    }
}

pub fn report_invalid_path(context: &Context, error: &TestPathError) {
    let builder = context.report_diagnostic(&INVALID_PATH);

    builder.into_diagnostic(format!("Invalid path: {error}"));
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

    diagnostic.annotate(Annotation::primary(primary_span));

    diagnostic.info(format!("Reason: {reason}"));
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

    diagnostic.annotate(Annotation::primary(primary_span));

    diagnostic.info(format!("Reason: {reason}"));
}

pub fn report_missing_fixtures(
    context: &Context,
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
    missing_fixtures: &[String],
    function_kind: FunctionKind,
) {
    let builder = context.report_diagnostic(&MISSING_FIXTURES);

    let mut diagnostic = builder.into_diagnostic(format!(
        "Discovered missing fixtures for {} `{}`",
        function_kind, stmt_function_def.name
    ));

    let primary_span = Span::from(source_file).with_range(stmt_function_def.name.range);

    diagnostic.annotate(Annotation::primary(primary_span));

    diagnostic.info(format!("Missing fixtures: {missing_fixtures:?}"));
}

pub fn report_fixture_failure(
    context: &Context,
    py: Python,
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
    error: &PyErr,
) {
    let builder = context.report_diagnostic(&FIXTURE_FAILURE);

    let mut diagnostic =
        builder.into_diagnostic(format!("Fixture `{}` failed", stmt_function_def.name));

    handle_failed_function_call(&mut diagnostic, py, source_file, stmt_function_def, error);
}

pub fn report_test_pass_on_expect_failure(
    context: &Context,
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
    reason: Option<String>,
) {
    let builder = context.report_diagnostic(&TEST_PASS_ON_EXPECT_FAILURE);

    let mut diagnostic = builder.into_diagnostic(format!(
        "Test `{}` passes when expected to fail",
        stmt_function_def.name
    ));

    let primary_span = Span::from(source_file).with_range(stmt_function_def.name.range);

    diagnostic.annotate(Annotation::primary(primary_span));

    if let Some(reason) = reason {
        diagnostic.info(format!("Reason: {reason}"));
    }
}

pub fn report_test_failure(
    context: &Context,
    py: Python,
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
    error: &PyErr,
) {
    let builder = context.report_diagnostic(&TEST_FAILURE);

    let mut diagnostic =
        builder.into_diagnostic(format!("Test `{}` failed", stmt_function_def.name));

    handle_failed_function_call(&mut diagnostic, py, source_file, stmt_function_def, error);
}

fn handle_failed_function_call(
    diagnostic: &mut Diagnostic,
    py: Python,
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
    error: &PyErr,
) {
    let primary_span = Span::from(source_file).with_range(stmt_function_def.name.range);

    diagnostic.annotate(Annotation::primary(primary_span));

    if let Some(Traceback {
        lines: _,
        error_source_file,
        location,
    }) = Traceback::from_error(py, error)
    {
        let mut sub = SubDiagnostic::new(SubDiagnosticSeverity::Info, "Test failed here");

        let secondary_span = Span::from(error_source_file).with_range(location);

        sub.annotate(Annotation::primary(secondary_span));

        diagnostic.sub(sub);
    }

    let error_string = error.value(py).to_string();

    if !error_string.is_empty() {
        diagnostic.info(format!("Error message: {error_string}"));
    }
}
