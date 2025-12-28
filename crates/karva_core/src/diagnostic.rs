//! Karva Diagnostics
//!
//! We use ruff diagnostics for all test diagnostics.
//!
//! Ruff diagnostics look great and have a great API.

use std::collections::HashMap;

use karva_diagnostic::Traceback;
use karva_python_semantic::FunctionKind;
use karva_system::path::TestPathError;
use pyo3::{Py, PyAny, PyErr, Python};
use ruff_db::diagnostic::{
    Annotation, Diagnostic, Severity, Span, SubDiagnostic, SubDiagnosticSeverity,
};
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

mod metadata;

pub use metadata::{DiagnosticGuardBuilder, DiagnosticType};

use crate::runner::FixtureCallError;
use crate::utils::truncate_string;
use crate::{Context, declare_diagnostic_type};

declare_diagnostic_type! {
    /// ## Invalid path
    ///
    /// User has provided an invalid path that we cannot resolve.
    pub static INVALID_PATH = {
        summary: "User provided an invalid path",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Failed to import module
    ///
    /// This comes from when we try to import tests or fixtures.
    /// If we try to import a module and it fails, we will raise this error.
    pub static FAILED_TO_IMPORT_MODULE = {
        summary: "Failed to import python module",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Invalid fixture
    ///
    /// There are several reasons a fixture may be invalid,
    /// we raise this error when we detect one.
    pub static INVALID_FIXTURE = {
        summary: "Discovered an invalid fixture",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Invalid fixture finalizer
    ///
    /// If a finalizer raises an exception, we will raise this error.
    /// If a finalizer tries to yield another value, we will raise this error.
    pub static INVALID_FIXTURE_FINALIZER = {
        summary: "Tried to run an invalid fixture finalizer",
        severity: Severity::Warning,
    }
}

declare_diagnostic_type! {
    /// ## Missing fixtures
    ///
    /// If we try to run a test or function without all the required fixtures,
    /// we will raise this error.
    pub static MISSING_FIXTURES = {
        summary: "Missing fixtures",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Failed Fixture
    ///
    /// If we call a fixture and it raises an exception, we will raise this error.
    pub static FIXTURE_FAILURE = {
        summary: "Fixture raises exception when run",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Test Passes when expected to fail
    ///
    /// If a test marked as `expect_failure` passes, we will raise this error.
    pub static TEST_PASS_ON_EXPECT_FAILURE = {
        summary: "Test passes when expected to fail",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Failed Test
    ///
    /// If a test raises an exception, we will raise this error.
    pub static TEST_FAILURE = {
        summary: "Test raises exception when run",
        severity: Severity::Error,
    }
}

pub fn report_invalid_path(context: &Context, error: &TestPathError) {
    let builder = context.report_discovery_diagnostic(&INVALID_PATH);

    builder.into_diagnostic(format!("Invalid path: {error}"));
}

pub fn report_failed_to_import_module(context: &Context, module_name: &str, error: &str) {
    let builder = context.report_discovery_diagnostic(&FAILED_TO_IMPORT_MODULE);

    builder.into_diagnostic(format!(
        "Failed to import python module `{module_name}`: {error}"
    ));
}

pub fn report_invalid_fixture(
    context: &Context,
    py: Python,
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
    error: &PyErr,
) {
    let builder = context.report_diagnostic(&INVALID_FIXTURE);

    let mut diagnostic = builder.into_diagnostic(format!(
        "Discovered an invalid fixture `{}`",
        stmt_function_def.name
    ));

    let primary_span = Span::from(source_file).with_range(stmt_function_def.name.range);

    diagnostic.annotate(Annotation::primary(primary_span));

    let error_string = error.value(py).to_string();

    if !error_string.is_empty() {
        diagnostic.info(error_string);
    }
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

    diagnostic.info(reason);
}

pub fn report_fixture_failure(context: &Context, py: Python, error: FixtureCallError) {
    let FixtureCallError {
        fixture_name,
        error,
        stmt_function_def,
        source_file,
        arguments,
    } = error;

    let builder = context.report_diagnostic(&FIXTURE_FAILURE);

    let mut diagnostic = builder.into_diagnostic(format!("Fixture `{fixture_name}` failed"));

    handle_failed_function_call(
        context,
        &mut diagnostic,
        py,
        source_file,
        &stmt_function_def,
        &arguments,
        FunctionKind::Fixture,
        &error,
    );
}

pub fn report_missing_fixtures(
    context: &Context,
    py: Python,
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
    missing_fixtures: &[String],
    function_kind: FunctionKind,
    fixture_call_errors: Vec<FixtureCallError>,
) {
    let builder = context.report_diagnostic(&MISSING_FIXTURES);

    let mut diagnostic = builder.into_diagnostic(format!(
        "{} `{}` has missing fixtures",
        function_kind.capitalised(),
        stmt_function_def.name
    ));

    let primary_span = Span::from(source_file).with_range(stmt_function_def.name.range);

    diagnostic.annotate(Annotation::primary(primary_span));

    let missing_fixtures_string = missing_fixtures
        .iter()
        .map(|fixture| format!("`{}`", truncate_string(fixture)))
        .collect::<Vec<String>>()
        .join(", ");

    diagnostic.info(format!("Missing fixtures: {missing_fixtures_string}"));

    diagnostic.set_concise_message(format!(
        "{} `{}` has missing fixtures: {missing_fixtures_string}",
        function_kind.capitalised(),
        stmt_function_def.name,
    ));

    for FixtureCallError {
        error,
        fixture_name,
        ..
    } in fixture_call_errors
    {
        if let Some(Traceback {
            lines: _,
            error_source_file,
            location,
        }) = Traceback::from_error(py, context.db().system(), &error)
        {
            let mut sub = SubDiagnostic::new(
                SubDiagnosticSeverity::Info,
                format!("Fixture `{fixture_name}` failed here"),
            );

            let secondary_span = Span::from(error_source_file).with_range(location);

            sub.annotate(Annotation::primary(secondary_span));

            diagnostic.sub(sub);
        }

        let error_string = error.value(py).to_string();

        if !error_string.is_empty() {
            diagnostic.info(error_string);
        }
    }
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
    arguments: &HashMap<String, Py<PyAny>>,
    error: &PyErr,
) {
    let builder = context.report_diagnostic(&TEST_FAILURE);

    let mut diagnostic =
        builder.into_diagnostic(format!("Test `{}` failed", stmt_function_def.name));

    handle_failed_function_call(
        context,
        &mut diagnostic,
        py,
        source_file,
        stmt_function_def,
        arguments,
        FunctionKind::Test,
        error,
    );
}

#[allow(clippy::too_many_arguments)]
fn handle_failed_function_call(
    context: &Context,
    diagnostic: &mut Diagnostic,
    py: Python,
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
    arguments: &HashMap<String, Py<PyAny>>,
    function_kind: FunctionKind,
    error: &PyErr,
) {
    let primary_span = Span::from(source_file).with_range(stmt_function_def.name.range);

    diagnostic.annotate(Annotation::primary(primary_span));

    if !arguments.is_empty() {
        diagnostic.info(format!(
            "{} ran with arguments:",
            function_kind.capitalised()
        ));
    }

    let mut sorted_arguments = arguments.iter().collect::<Vec<_>>();
    sorted_arguments.sort_by_key(|&(name, _)| name);

    for (name, value) in sorted_arguments {
        let value_str = value.bind(py).to_string();
        let truncated_value = truncate_string(&value_str);
        let truncated_name = truncate_string(name);
        diagnostic.info(format!("`{truncated_name}`: `{truncated_value}`"));
    }

    if let Some(Traceback {
        lines: _,
        error_source_file,
        location,
    }) = Traceback::from_error(py, context.db().system(), error)
    {
        let mut sub = SubDiagnostic::new(
            SubDiagnosticSeverity::Info,
            format!("{} failed here", function_kind.capitalised()),
        );

        let secondary_span = Span::from(error_source_file).with_range(location);

        sub.annotate(Annotation::primary(secondary_span));

        diagnostic.sub(sub);
    }

    let error_string = error.value(py).to_string();

    if !error_string.is_empty() {
        diagnostic.info(error_string);
    }
}
