//! Karva Diagnostics
//!
//! We use `ruff_db` diagnostics for all test diagnostics.
//!
//! `ruff_db` diagnostics look great and have a great API.

use std::collections::HashMap;

use karva_collector::CollectionError;
use karva_diagnostic::Traceback;
use karva_python_semantic::FunctionKind;
use pyo3::{Py, PyAny, PyErr, Python};
use ruff_db::diagnostic::{
    Annotation, Diagnostic, Severity, Span, SubDiagnostic, SubDiagnosticSeverity,
};
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

mod metadata;

pub use metadata::DiagnosticBuilder;
pub use metadata::DiagnosticType;

use crate::runner::{FixtureCallError, FixtureChainEntry};
use crate::utils::truncate_string;
use crate::{Context, declare_diagnostic_type};

declare_diagnostic_type! {
    /// ## Failed to collect module
    ///
    /// Raised when karva cannot read a Python file during collection. This is
    /// distinct from import failures: the file could not be parsed or imported
    /// because its source text was unavailable.
    pub static FAILED_TO_COLLECT_MODULE = {
        summary: "Failed to collect python module",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Failed to import module
    ///
    /// Raised when karva tries to import a test module or fixture file and the
    /// import itself fails (e.g. a syntax error, an unresolved import, an
    /// exception at top level). Any tests inside the module are not discovered;
    /// successfully-collected tests in other modules still run, but the run
    /// exits non-zero.
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
        severity: Severity::Error,
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

/// Annotate a diagnostic with a primary span pointing at a function's name.
fn annotate_function_name(
    diagnostic: &mut Diagnostic,
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
) {
    let span = Span::from(source_file).with_range(stmt_function_def.name.range);
    diagnostic.annotate(Annotation::primary(span));
}

/// Emits sub-diagnostics for each intermediate fixture in the dependency chain,
/// showing a span annotation for each fixture between the test and the one that failed.
fn report_dependency_chain(
    diagnostic: &mut Diagnostic,
    dependency_chain: &[FixtureChainEntry],
    fixture_name: &str,
) {
    // Walk the chain top-down, pairing each entry with the fixture it depends on.
    // The final entry depends on `fixture_name` (the one that actually failed).
    let mut entries = dependency_chain.iter().rev().peekable();
    while let Some(entry) = entries.next() {
        let next_name = entries
            .peek()
            .map_or(fixture_name, |next| next.name.as_str());

        let mut sub = SubDiagnostic::new(
            SubDiagnosticSeverity::Info,
            format!("Fixture `{}` requires `{next_name}`", entry.name),
        );

        let span =
            Span::from(entry.source_file.clone()).with_range(entry.stmt_function_def.name.range);

        sub.annotate(Annotation::primary(span));
        diagnostic.sub(sub);
    }
}

pub fn report_collection_error(context: &Context, error: &CollectionError) {
    let builder = context.report_diagnostic(&FAILED_TO_COLLECT_MODULE);

    builder.emit(format!("Failed to collect python module: {error}"));
}

pub fn report_failed_to_import_module(context: &Context, module_name: &str, error: &str) {
    let builder = context.report_diagnostic(&FAILED_TO_IMPORT_MODULE);

    builder.emit(format!(
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

    builder.emit_with(
        format!("Discovered an invalid fixture `{}`", stmt_function_def.name),
        |diagnostic| {
            annotate_function_name(diagnostic, source_file, stmt_function_def);

            let error_string = error.value(py).to_string();
            if !error_string.is_empty() {
                diagnostic.info(indent_continuation_lines(&error_string));
            }
        },
    );
}

pub fn report_invalid_fixture_finalizer(
    context: &Context,
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
    reason: &str,
) {
    let builder = context.report_diagnostic(&INVALID_FIXTURE_FINALIZER);

    builder.emit_with(
        format!(
            "Discovered an invalid fixture finalizer `{}`",
            stmt_function_def.name
        ),
        |diagnostic| {
            annotate_function_name(diagnostic, source_file, stmt_function_def);
            diagnostic.info(reason);
        },
    );
}

pub fn report_fixture_failure(context: &Context, py: Python, error: FixtureCallError) {
    let FixtureCallError {
        fixture_name,
        error,
        stmt_function_def,
        source_file,
        arguments,
        dependency_chain,
    } = error;

    let builder = context.report_diagnostic(&FIXTURE_FAILURE);

    builder.emit_with(format!("Fixture `{fixture_name}` failed"), |diagnostic| {
        report_dependency_chain(diagnostic, &dependency_chain, &fixture_name);

        handle_failed_function_call(
            diagnostic,
            py,
            &source_file,
            &stmt_function_def,
            &arguments,
            FunctionKind::Fixture,
            &error,
        );
    });
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

    let missing_fixtures_string = missing_fixtures
        .iter()
        .map(|fixture| format!("`{}`", truncate_string(fixture)))
        .collect::<Vec<String>>()
        .join(", ");

    builder.emit_with(
        format!(
            "{} `{}` has missing fixtures",
            function_kind.capitalised(),
            stmt_function_def.name
        ),
        |diagnostic| {
            annotate_function_name(diagnostic, source_file, stmt_function_def);

            diagnostic.info(format!("Missing fixtures: {missing_fixtures_string}"));

            diagnostic.set_concise_message(format!(
                "{} `{}` has missing fixtures: {missing_fixtures_string}",
                function_kind.capitalised(),
                stmt_function_def.name,
            ));

            for FixtureCallError {
                error,
                fixture_name,
                source_file,
                dependency_chain,
                ..
            } in fixture_call_errors
            {
                report_dependency_chain(diagnostic, &dependency_chain, &fixture_name);

                if let Some(Traceback {
                    lines: _,
                    error_source_file,
                    location,
                }) = Traceback::from_error_with_source(py, &error, &source_file)
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
                    diagnostic.info(indent_continuation_lines(&error_string));
                }
            }
        },
    );
}

pub fn report_test_pass_on_expect_failure(
    context: &Context,
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
    reason: Option<String>,
) {
    let builder = context.report_diagnostic(&TEST_PASS_ON_EXPECT_FAILURE);

    builder.emit_with(
        format!(
            "Test `{}` passes when expected to fail",
            stmt_function_def.name
        ),
        |diagnostic| {
            annotate_function_name(diagnostic, source_file, stmt_function_def);

            if let Some(reason) = reason {
                diagnostic.info(format!("Reason: {reason}"));
            }
        },
    );
}

pub fn report_test_failure(
    context: &Context,
    py: Python,
    source_file: &SourceFile,
    stmt_function_def: &StmtFunctionDef,
    arguments: &HashMap<String, Py<PyAny>>,
    error: &PyErr,
) {
    let builder = context.report_diagnostic(&TEST_FAILURE);

    builder.emit_with(
        format!("Test `{}` failed", stmt_function_def.name),
        |diagnostic| {
            handle_failed_function_call(
                diagnostic,
                py,
                source_file,
                stmt_function_def,
                arguments,
                FunctionKind::Test,
                error,
            );
        },
    );
}

fn handle_failed_function_call(
    diagnostic: &mut Diagnostic,
    py: Python,
    source_file: &SourceFile,
    stmt_function_def: &StmtFunctionDef,
    arguments: &HashMap<String, Py<PyAny>>,
    function_kind: FunctionKind,
    error: &PyErr,
) {
    annotate_function_name(diagnostic, source_file.clone(), stmt_function_def);

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
    }) = Traceback::from_error_with_source(py, error, source_file)
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
        diagnostic.info(indent_continuation_lines(&error_string));
    }
}

/// Indent continuation lines in a multi-line message so they align under the first line's text.
///
/// The first line is left as-is (it appears after `info: ` in diagnostic output).
/// Subsequent lines are indented with 6 spaces to align with the first line's text.
fn indent_continuation_lines(message: &str) -> String {
    let mut lines = message.lines();
    let Some(first) = lines.next() else {
        return String::new();
    };

    let mut result = first.to_string();
    for line in lines {
        result.push('\n');
        if !line.is_empty() {
            result.push_str("      ");
        }
        result.push_str(line);
    }
    result
}
