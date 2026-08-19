//! Converts collection and execution failures into source-backed diagnostics.
//!
//! Discovery and execution failures are rendered here without mutating run state.

use camino::Utf8Path;
use karva_collector::CollectionError;
use karva_diagnostic::{
    Annotation, Diagnostic, Severity, Span, SubDiagnostic, Traceback, TracebackFrame,
};
use karva_logging::time::format_duration;
use karva_python_semantic::FunctionKind;
use pyo3::{PyErr, Python};
use ruff_python_ast::{Parameters, StmtFunctionDef};
use ruff_source_file::{OneIndexed, SourceFile};
use ruff_text_size::{TextRange, TextSize};

mod metadata;

use crate::declare_diagnostic_type;
use crate::discovery::models::definition::TestDefinition;
use crate::extensions::fixtures::RejectedFixture;
use crate::extensions::functions::SnapshotMismatchError;
use crate::extensions::tags::parametrize::InvalidParametrizeError;
use crate::runner::{
    FixtureArguments, FixtureCallError, FixtureChainEntry, FixtureResolutionEntry,
    FixtureResolutionError,
};
use crate::utils::truncate_string;

#[derive(Clone, Copy)]
struct FailedFunctionCallOptions {
    function_kind: FunctionKind,
    verbose: bool,
    primary_range: Option<TextRange>,
}

#[derive(Clone, Copy)]
struct FailedFunctionDefinition<'a> {
    source_file: &'a SourceFile,
    range: TextRange,
    parameters: Option<&'a Parameters>,
}

impl<'a> FailedFunctionDefinition<'a> {
    fn function(source_file: &'a SourceFile, statement: &'a StmtFunctionDef) -> Self {
        Self {
            source_file,
            range: statement.name.range,
            parameters: Some(statement.parameters.as_ref()),
        }
    }

    fn test(definition: &'a TestDefinition) -> Self {
        Self {
            source_file: definition.source_file(),
            range: definition.diagnostic_range(),
            parameters: definition.parameters(),
        }
    }
}

declare_diagnostic_type! {
    /// ## Unknown tag
    ///
    /// Raised during strict tag validation before the module is imported.
    pub static UNKNOWN_TAG = {
        summary: "Unknown custom tag",
        severity: Severity::Error,
    }
}

pub fn unknown_tag_diagnostic(
    source_file: SourceFile,
    name: &str,
    range: TextRange,
    suggestion: Option<&str>,
) -> Diagnostic {
    let mut diagnostic = UNKNOWN_TAG.diagnostic(format!("Tag `{name}` is not registered"));
    diagnostic.annotate(
        Annotation::primary(Span::from(source_file).with_range(range)).message("unregistered tag"),
    );
    if let Some(suggestion) = suggestion {
        diagnostic.info(format!("Did you mean `{suggestion}`?"));
    } else {
        diagnostic.info(format!(
            "Register `{name}` in the project-wide `[tags]` table."
        ));
    }
    diagnostic
}

declare_diagnostic_type! {
    /// ## Failed to start coverage
    ///
    /// Raised when configured coverage sources cannot be resolved by the
    /// worker's Python environment.
    pub static FAILED_TO_START_COVERAGE = {
        summary: "Failed to start coverage measurement",
        severity: Severity::Error,
    }
}

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

pub fn failed_to_start_coverage_diagnostic(reason: &str) -> Diagnostic {
    FAILED_TO_START_COVERAGE.diagnostic(format!("Failed to start coverage measurement: {reason}"))
}

declare_diagnostic_type! {
    /// ## Invalid parametrization
    ///
    /// Raised when a parametrization cannot produce valid test arguments.
    pub static INVALID_PARAMETRIZE = {
        summary: "Invalid parametrization",
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
    /// ## Failed to discover imported fixture
    ///
    /// Raised when karva finds a fixture object imported into a test module or
    /// `conftest.py`, but cannot read the original source file needed to locate
    /// the fixture definition.
    pub static FAILED_TO_DISCOVER_IMPORTED_FIXTURE = {
        summary: "Failed to discover imported fixture",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Duplicate fixture
    ///
    /// Raised when a module defines the same fixture more than once.
    pub static DUPLICATE_FIXTURE = {
        summary: "Discovered duplicate fixture definitions",
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
    /// ## Fixture dependency cycle
    ///
    /// Raised when fixture dependencies form a cycle and cannot be resolved.
    pub static FIXTURE_CYCLE = {
        summary: "Fixture dependency cycle detected",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Fixture scope mismatch
    ///
    /// Raised when a fixture depends on another fixture with a shorter lifetime.
    pub static FIXTURE_SCOPE_MISMATCH = {
        summary: "Fixture scope mismatch detected",
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

declare_diagnostic_type! {
    /// ## Duplicate test
    ///
    /// Raised when a module defines the same test function more than once.
    pub static DUPLICATE_TEST = {
        summary: "Discovered duplicate test definitions",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Invalid Test
    ///
    /// If a test definition cannot be run by Karva, we will raise this error.
    pub static INVALID_TEST = {
        summary: "Discovered an invalid test",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Test Returned Value
    ///
    /// If a test returns anything other than `None`, we will raise this error.
    pub static TEST_RETURNED_VALUE = {
        summary: "Test returned a non-None value",
        severity: Severity::Error,
    }
}

declare_diagnostic_type! {
    /// ## Fail-slow budget exceeded
    ///
    /// Raised when a test's full lifecycle (fixture setup, the test call,
    /// and fixture teardown) takes longer than its configured `fail-slow`
    /// budget. Unlike a hard `timeout`, the test always runs to completion
    /// before this diagnostic is produced.
    pub static FAIL_SLOW_EXCEEDED = {
        summary: "Test exceeded its fail-slow duration budget",
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

fn annotate_test(diagnostic: &mut Diagnostic, definition: &TestDefinition) {
    let span =
        Span::from(definition.source_file().clone()).with_range(definition.diagnostic_range());
    diagnostic.annotate(Annotation::primary(span));
}

fn annotate_first_definition(
    diagnostic: &mut Diagnostic,
    source_file: SourceFile,
    name: &str,
    stmt_function_def: &StmtFunctionDef,
) {
    let mut sub = SubDiagnostic::new(
        Severity::Info,
        format!("First definition of `{name}` is here"),
    );
    let span = Span::from(source_file).with_range(stmt_function_def.name.range);
    sub.annotate(Annotation::primary(span));
    diagnostic.sub(sub);
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
            Severity::Info,
            format!("Fixture `{}` requires `{next_name}`", entry.name),
        );

        let span = Span::from(entry.definition.source_file().clone())
            .with_range(entry.definition.statement().name.range);

        sub.annotate(Annotation::primary(span));
        diagnostic.sub(sub);
    }
}

pub fn collection_error_diagnostic(error: &CollectionError) -> Diagnostic {
    FAILED_TO_COLLECT_MODULE.diagnostic(format!("Failed to collect python module: {error}"))
}

pub fn failed_to_import_module_diagnostic(module_name: &str, error: &str) -> Diagnostic {
    FAILED_TO_IMPORT_MODULE.diagnostic(format!(
        "Failed to import python module `{module_name}`: {error}"
    ))
}

pub fn failed_to_discover_imported_fixture_diagnostic(
    fixture_name: &str,
    source_path: &Utf8Path,
    error: &std::io::Error,
) -> Diagnostic {
    FAILED_TO_DISCOVER_IMPORTED_FIXTURE.diagnostic(format!(
        "Failed to discover imported fixture `{fixture_name}` from `{source_path}`: {error}"
    ))
}

pub fn duplicate_fixture_diagnostic(
    source_file: SourceFile,
    fixture_name: &str,
    first_definition: &StmtFunctionDef,
    duplicate_definition: &StmtFunctionDef,
) -> Diagnostic {
    let mut diagnostic = DUPLICATE_FIXTURE.diagnostic(format!(
        "Fixture `{fixture_name}` is defined more than once"
    ));

    annotate_function_name(&mut diagnostic, source_file.clone(), duplicate_definition);
    annotate_first_definition(&mut diagnostic, source_file, fixture_name, first_definition);
    diagnostic
}

pub fn duplicate_test_diagnostic(
    source_file: SourceFile,
    test_name: &str,
    first_definition: &StmtFunctionDef,
    duplicate_definition: &StmtFunctionDef,
) -> Diagnostic {
    let mut diagnostic =
        DUPLICATE_TEST.diagnostic(format!("Test `{test_name}` is defined more than once"));

    annotate_function_name(&mut diagnostic, source_file.clone(), duplicate_definition);
    annotate_first_definition(&mut diagnostic, source_file, test_name, first_definition);
    diagnostic
}

pub fn invalid_fixture_diagnostic(
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
    reason: &str,
) -> Diagnostic {
    let mut diagnostic = INVALID_FIXTURE.diagnostic(format!(
        "Discovered an invalid fixture `{}`",
        stmt_function_def.name
    ));

    annotate_function_name(&mut diagnostic, source_file, stmt_function_def);

    if !reason.is_empty() {
        diagnostic.info(indent_continuation_lines(reason));
    }
    diagnostic
}

pub fn invalid_fixture_finalizer_diagnostic(
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
    reason: &str,
) -> Diagnostic {
    let mut diagnostic = INVALID_FIXTURE_FINALIZER.diagnostic(format!(
        "Discovered an invalid fixture finalizer `{}`",
        stmt_function_def.name
    ));

    annotate_function_name(&mut diagnostic, source_file, stmt_function_def);

    diagnostic.info(reason);
    diagnostic
}

pub fn fixture_failure_diagnostic(
    py: Python,
    error: FixtureCallError,
    verbose: bool,
) -> Diagnostic {
    let FixtureCallError {
        fixture_name,
        error,
        definition,
        arguments,
        dependency_chain,
    } = error;

    let mut diagnostic = FIXTURE_FAILURE.diagnostic(format!("Fixture `{fixture_name}` failed"));

    report_dependency_chain(&mut diagnostic, &dependency_chain, &fixture_name);

    handle_failed_function_call(
        &mut diagnostic,
        py,
        FailedFunctionDefinition::function(definition.source_file(), definition.statement()),
        &arguments,
        FailedFunctionCallOptions {
            function_kind: FunctionKind::Fixture,
            verbose,
            primary_range: None,
        },
        &error,
    );
    diagnostic
}

pub fn fixture_resolution_diagnostic(error: FixtureResolutionError) -> Diagnostic {
    match error {
        FixtureResolutionError::Cycle { cycle } => fixture_cycle_diagnostic(&cycle),
        FixtureResolutionError::ScopeMismatch {
            dependency_path,
            fixture,
            dependency,
        } => fixture_scope_mismatch_diagnostic(&dependency_path, &fixture, &dependency),
        FixtureResolutionError::MissingFixtures {
            fixture,
            missing_fixtures,
            rejected_fixtures,
        } => fixture_missing_fixtures_diagnostic(&fixture, &missing_fixtures, &rejected_fixtures),
        FixtureResolutionError::MissingTestFixtures {
            definition,
            missing_fixtures,
        } => missing_fixtures_diagnostic(
            definition.source_file().clone(),
            definition.name().function_name(),
            definition.diagnostic_range(),
            &missing_fixtures,
            FunctionKind::Test,
        ),
    }
}

fn fixture_cycle_diagnostic(cycle: &[FixtureResolutionEntry]) -> Diagnostic {
    let mut diagnostic = FIXTURE_CYCLE.diagnostic("Fixture dependency cycle detected");

    if let Some(first_fixture) = cycle.first() {
        annotate_function_name(
            &mut diagnostic,
            first_fixture.definition.source_file().clone(),
            first_fixture.definition.statement(),
        );
    }

    for dependency_edge in cycle.windows(2).skip(1) {
        let [fixture, dependency] = dependency_edge else {
            continue;
        };
        let mut sub = SubDiagnostic::new(
            Severity::Info,
            format!("Fixture `{}` requires `{}`", fixture.name, dependency.name),
        );
        let span = Span::from(fixture.definition.source_file().clone())
            .with_range(fixture.definition.statement().name.range);
        sub.annotate(Annotation::primary(span));
        diagnostic.sub(sub);
    }

    diagnostic.info(
        cycle
            .iter()
            .map(|fixture| fixture.name.as_str())
            .collect::<Vec<_>>()
            .join(" -> "),
    );
    diagnostic
}

fn fixture_scope_mismatch_diagnostic(
    dependency_path: &[FixtureResolutionEntry],
    fixture: &FixtureResolutionEntry,
    dependency: &FixtureResolutionEntry,
) -> Diagnostic {
    let mut diagnostic = FIXTURE_SCOPE_MISMATCH.diagnostic(format!(
        "Fixture `{}` with `{}` scope cannot depend on fixture `{}` with `{}` scope",
        fixture.name,
        fixture.scope.name(),
        dependency.name,
        dependency.scope.name(),
    ));

    annotate_function_name(
        &mut diagnostic,
        fixture.definition.source_file().clone(),
        fixture.definition.statement(),
    );

    for (index, path_fixture) in dependency_path.iter().enumerate() {
        let next_fixture = dependency_path.get(index + 1).unwrap_or(fixture);
        let mut sub = SubDiagnostic::new(
            Severity::Info,
            format!(
                "Fixture `{}` depends on fixture `{}`",
                path_fixture.name, next_fixture.name
            ),
        );
        let span = Span::from(path_fixture.definition.source_file().clone())
            .with_range(path_fixture.definition.statement().name.range);
        sub.annotate(Annotation::primary(span));
        diagnostic.sub(sub);
    }

    let mut dependency_sub = SubDiagnostic::new(
        Severity::Info,
        format!(
            "Fixture `{}` has `{}` scope",
            dependency.name,
            dependency.scope.name()
        ),
    );
    let span = Span::from(dependency.definition.source_file().clone())
        .with_range(dependency.definition.statement().name.range);
    dependency_sub.annotate(Annotation::primary(span));
    diagnostic.sub(dependency_sub);
    diagnostic
}

fn fixture_missing_fixtures_diagnostic(
    fixture: &FixtureResolutionEntry,
    missing_fixtures: &[String],
    rejected_fixtures: &[RejectedFixture],
) -> Diagnostic {
    let mut diagnostic = missing_fixtures_diagnostic(
        fixture.definition.source_file().clone(),
        fixture.definition.statement().name.as_str(),
        fixture.definition.statement().name.range,
        missing_fixtures,
        FunctionKind::Fixture,
    );

    for rejected_fixture in rejected_fixtures {
        let mut sub = SubDiagnostic::new(
            Severity::Info,
            format!(
                "Fixture `{}` was rejected during discovery: {}",
                rejected_fixture.exposure_name(),
                rejected_fixture.reason()
            ),
        );
        let span = Span::from(rejected_fixture.source_file().clone())
            .with_range(rejected_fixture.statement().name.range);
        sub.annotate(Annotation::primary(span));
        diagnostic.sub(sub);
    }

    diagnostic
}

pub fn missing_fixtures_diagnostic(
    source_file: SourceFile,
    name: &str,
    range: TextRange,
    missing_fixtures: &[String],
    function_kind: FunctionKind,
) -> Diagnostic {
    let mut diagnostic = MISSING_FIXTURES.diagnostic(format!(
        "{} `{name}` has missing fixtures",
        function_kind.capitalised(),
    ));

    diagnostic.annotate(Annotation::primary(
        Span::from(source_file).with_range(range),
    ));

    let missing_fixtures_string = missing_fixtures
        .iter()
        .map(|fixture| format!("`{}`", truncate_string(fixture)))
        .collect::<Vec<String>>()
        .join(", ");

    diagnostic.info(format!("Missing fixtures: {missing_fixtures_string}"));

    diagnostic.set_concise_message(format!(
        "{} `{name}` has missing fixtures: {missing_fixtures_string}",
        function_kind.capitalised(),
    ));

    diagnostic
}

pub fn test_pass_on_expect_failure_diagnostic(
    definition: &TestDefinition,
    reason: Option<String>,
) -> Diagnostic {
    let mut diagnostic = TEST_PASS_ON_EXPECT_FAILURE.diagnostic(format!(
        "Test `{}` passes when expected to fail",
        definition.name().function_name()
    ));

    annotate_test(&mut diagnostic, definition);

    if let Some(reason) = reason {
        diagnostic.info(format!("Reason: {reason}"));
    }
    diagnostic
}

pub fn test_failure_diagnostic(
    py: Python,
    definition: &TestDefinition,
    arguments: &FixtureArguments,
    error: &PyErr,
    verbose: bool,
) -> Diagnostic {
    let mut diagnostic = TEST_FAILURE.diagnostic(format!(
        "Test `{}` failed",
        definition.name().function_name()
    ));

    handle_failed_function_call(
        &mut diagnostic,
        py,
        FailedFunctionDefinition::test(definition),
        arguments,
        FailedFunctionCallOptions {
            function_kind: FunctionKind::Test,
            verbose,
            primary_range: doctest_failure_range(py, error, definition.source_file()),
        },
        error,
    );
    diagnostic
}

pub fn generator_test_diagnostic(
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
) -> Diagnostic {
    let mut diagnostic = INVALID_TEST.diagnostic(format!(
        "Generator test `{}` is not supported",
        stmt_function_def.name
    ));

    annotate_function_name(&mut diagnostic, source_file, stmt_function_def);
    diagnostic.info("Use `@karva.tags.parametrize` to define multiple test cases.");
    diagnostic
}

pub fn invalid_parametrize_diagnostic(
    source_file: SourceFile,
    stmt_function_def: &StmtFunctionDef,
    error: &InvalidParametrizeError,
) -> Diagnostic {
    let mut diagnostic = INVALID_PARAMETRIZE.diagnostic(error.to_string());
    let Some(location) = error.diagnostic_location(stmt_function_def) else {
        annotate_function_name(&mut diagnostic, source_file, stmt_function_def);
        return diagnostic;
    };

    diagnostic.annotate(
        Annotation::primary(Span::from(source_file.clone()).with_range(location.primary.range))
            .message(location.primary.message),
    );
    for secondary in location.secondary {
        diagnostic.annotate(
            Annotation::secondary(Span::from(source_file.clone()).with_range(secondary.range))
                .message(secondary.message),
        );
    }
    diagnostic
}

pub fn test_returned_value_diagnostic(
    definition: &TestDefinition,
    returned_value: &str,
) -> Diagnostic {
    let mut diagnostic = TEST_RETURNED_VALUE.diagnostic(format!(
        "Test `{}` returned `{returned_value}`",
        definition.name().function_name()
    ));

    annotate_test(&mut diagnostic, definition);
    diagnostic.info("Test functions must return None. Did you mean to use `assert`?");
    diagnostic
}

/// Build the diagnostic for a test whose full lifecycle exceeded its
/// configured `fail-slow` budget.
///
/// `actual` and `slowest_phase` describe the measured setup, call, and
/// teardown phases for one attempt.
pub fn fail_slow_exceeded_diagnostic(
    definition: &TestDefinition,
    budget: std::time::Duration,
    actual: std::time::Duration,
    slowest_phase: &str,
) -> Diagnostic {
    let mut diagnostic = FAIL_SLOW_EXCEEDED.diagnostic(format!(
        "Test `{}` exceeded its fail-slow budget",
        definition.name().function_name()
    ));

    annotate_test(&mut diagnostic, definition);

    diagnostic.info(format!(
        "Configured budget: {}, actual duration: {} (slowest phase: {slowest_phase})",
        format_duration(budget),
        format_duration(actual),
    ));

    diagnostic
}

fn handle_failed_function_call(
    diagnostic: &mut Diagnostic,
    py: Python,
    definition: FailedFunctionDefinition<'_>,
    arguments: &FixtureArguments,
    options: FailedFunctionCallOptions,
    error: &PyErr,
) {
    let FailedFunctionCallOptions {
        function_kind,
        verbose,
        primary_range,
    } = options;
    let FailedFunctionDefinition {
        source_file,
        range,
        parameters,
    } = definition;
    diagnostic.annotate(Annotation::primary(
        Span::from(source_file.clone()).with_range(primary_range.unwrap_or(range)),
    ));

    if !arguments.is_empty() {
        diagnostic.info(format!(
            "{} ran with arguments:",
            function_kind.capitalised()
        ));
    }

    for (name, value) in arguments.iter_in_signature_order(parameters) {
        let value_str = value.bind(py).to_string();
        let truncated_value = truncate_string(&value_str);
        let truncated_name = truncate_string(name);
        let argument = SubDiagnostic::new(
            Severity::Info,
            format!("  `{truncated_name}`: `{truncated_value}`"),
        );
        diagnostic.sub(argument);
    }

    if let Some(Traceback { frames }) = Traceback::from_error_with_source(py, error, source_file) {
        if verbose {
            for pair in frames.windows(2) {
                if let [caller, callee] = pair {
                    let message = format!("Called `{}` here", callee.function_name);
                    if let Some(source) = &caller.source {
                        let mut sub = SubDiagnostic::new(Severity::Info, message);
                        sub.annotate(Annotation::primary(
                            Span::from(source.source_file.clone()).with_range(source.location),
                        ));
                        diagnostic.sub(sub);
                    } else {
                        diagnostic.info(format!(
                            "{message}: {}:{}",
                            caller.file_path, caller.line_number
                        ));
                    }
                }
            }
        }

        let failure = if verbose {
            frames.last()
        } else {
            frames
                .iter()
                .rev()
                .find(|frame| !is_installed_package_frame(frame))
                .or_else(|| frames.last())
        };

        if let Some(failure) = failure {
            let message = format!("{} failed here", function_kind.capitalised());
            if let Some(source) = &failure.source {
                let mut sub = SubDiagnostic::new(Severity::Info, message);
                sub.annotate(Annotation::primary(
                    Span::from(source.source_file.clone()).with_range(source.location),
                ));
                diagnostic.sub(sub);
            } else if verbose {
                diagnostic.info(format!(
                    "{message}: {}:{}",
                    failure.file_path, failure.line_number
                ));
            }
        }
    }

    let error_string = error.value(py).to_string();

    if !error_string.is_empty() {
        if error.is_instance_of::<SnapshotMismatchError>(py)
            && let Some((message, body)) = error_string.split_once('\n')
        {
            let (body, hint) = if let Some(hint) = body.strip_prefix("info: ") {
                ("", Some(hint))
            } else if let Some((body, hint)) = body.rsplit_once("\ninfo: ") {
                (body, Some(hint))
            } else {
                (body, None)
            };
            let mut mismatch = SubDiagnostic::new(Severity::Info, message);
            if !body.is_empty() {
                mismatch.body(body);
            }
            diagnostic.sub(mismatch);
            if let Some(hint) = hint {
                diagnostic.info(hint);
            }
        } else {
            diagnostic.info(indent_continuation_lines(&error_string));
        }
    }
}

fn doctest_failure_range(py: Python, error: &PyErr, source_file: &SourceFile) -> Option<TextRange> {
    let line = crate::doctest::failure_line(py, error)?;
    let line = OneIndexed::new(line)?;
    let source = source_file.to_source_code();
    if line.get() > source.line_count() {
        return None;
    }
    let line_range = source.line_range(line);
    let prompt = source.slice(line_range).find(">>>")?;
    let prompt = TextSize::try_from(prompt).ok()?;
    Some(TextRange::at(line_range.start() + prompt, TextSize::new(3)))
}

fn is_installed_package_frame(frame: &TracebackFrame) -> bool {
    frame
        .file_path
        .components()
        .any(|component| matches!(component.as_str(), "site-packages" | "dist-packages"))
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
