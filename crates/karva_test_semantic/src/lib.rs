//! Python test semantics, fixture resolution, extensions, and in-process execution.

use std::collections::{BTreeMap, BTreeSet};

mod assertions;
pub(crate) mod collection;
mod context;
pub(crate) mod diagnostic;
pub(crate) mod discovery;
mod doctest;
pub(crate) mod extensions;
mod output_capture;
mod py_attach;
mod python;
mod runner;
mod unhandled_exceptions;
mod utils;
mod warnings;

pub(crate) use context::{Context, RunState};
pub use karva_coverage::CoverageConfig;
pub use python::init_module;

use camino::Utf8Path;
use karva_coverage::CoverageSession;
use karva_diagnostic::{Diagnostic, Reporter};
use karva_metadata::ProjectSettings;
use karva_project::path::{TestPath, TestPathError};
use karva_python_semantic::TestCacheKey;
use ruff_python_ast::PythonVersion;

use crate::diagnostic::{
    exception_hook_diagnostic, failed_to_start_coverage_diagnostic, unhandled_exception_diagnostic,
};
use crate::discovery::{DiscoveryIssue, StandardDiscoverer};
use crate::py_attach::attach_with_output;
use crate::runner::PackageRunner;
use crate::unhandled_exceptions::UnhandledExceptionCapture;

/// Inputs needed to discover and execute one worker's test selection.
///
/// Borrowed values remain owned by the worker for the duration of the run;
/// `test_paths` is consumed by discovery. Coverage startup failures become run
/// diagnostics; persistence failures are traced without discarding results.
pub struct RunRequest<'a> {
    /// Current working directory used to resolve paths and render diagnostics.
    pub cwd: &'a Utf8Path,

    /// Project-level configuration that controls collection and execution.
    pub settings: &'a ProjectSettings,

    /// Python grammar target matching the embedded interpreter.
    pub python_version: PythonVersion,

    /// Reporter receiving test lifecycle events and results.
    pub reporter: &'a dyn Reporter,

    /// Test paths assigned to this worker, including paths that failed parsing.
    pub test_paths: Vec<Result<TestPath, TestPathError>>,

    /// Cases already committed by an earlier worker generation and therefore
    /// safe to omit during crash recovery.
    pub resume_skip: &'a BTreeSet<TestCacheKey>,

    /// Next attempt numbers for hard-timeout retries in this fresh worker.
    pub resume_attempts: &'a BTreeMap<TestCacheKey, u32>,

    /// Optional coverage configuration for this worker's run.
    pub coverage: Option<&'a CoverageConfig>,

    /// Whether diagnostics should include the full Python call chain.
    pub verbose: bool,
}

/// Runs discovery and execution inside one interpreter attachment.
pub fn run_tests(request: RunRequest<'_>) -> Vec<Diagnostic> {
    let context = Context::from_request(&request);
    let RunRequest {
        settings,
        test_paths,
        coverage,
        ..
    } = request;
    let mut state = RunState::default();

    attach_with_output(settings.terminal().show_python_output, |py| {
        let warnings = match warnings::WarningCapture::start(py, &settings.test().warning_filters) {
            Ok(capture) => capture,
            Err(error) => {
                state.add_run_diagnostic(warnings::warning_capture_error(&error));
                return state.into_result();
            }
        };
        let exception_capture = match UnhandledExceptionCapture::start(py) {
            Ok(capture) => capture,
            Err(error) => {
                state.add_run_diagnostic(exception_hook_diagnostic("install", &error.to_string()));
                if let Err(error) = warnings.finish(py, &context, None, None) {
                    state.add_run_diagnostic(warnings::warning_capture_error(&error));
                }
                return state.into_result();
            }
        };
        if let Err(error) = assertions::install(py, context.cwd()) {
            tracing::warn!("failed to install assertion instrumentation: {error}");
        }

        let cov_session =
            coverage.and_then(|cfg| match CoverageSession::start(py, context.cwd(), cfg) {
                Ok(session) => Some(session),
                Err(err) => {
                    state.add_run_diagnostic(failed_to_start_coverage_diagnostic(
                        &err.value(py).to_string(),
                    ));
                    None
                }
            });

        let discovery = StandardDiscoverer::new(&context).discover_with_py(py, test_paths);

        for issue in discovery.issues {
            match issue {
                DiscoveryIssue::Error(error) => {
                    state.add_run_diagnostic(error.into_diagnostic());
                }
                DiscoveryIssue::SkippedModule {
                    module_path,
                    reason,
                } => context.register_module_skip(&module_path, reason),
            }
        }

        PackageRunner::new(
            &context,
            &mut state,
            cov_session.as_ref(),
            &exception_capture,
        )
        .execute(py, &discovery.package);

        if let Some(cov_session) = cov_session
            && let Err(err) = cov_session.stop_and_save(py)
        {
            tracing::error!("Failed to save coverage data: {err}");
        }

        // Releasing imported modules and cached Python values can invoke __del__.
        drop(discovery.package);
        if let Err(error) = warnings.finish(py, &context, None, None) {
            state.add_run_diagnostic(warnings::warning_capture_error(&error));
        }
        let (events, errors) = exception_capture.finish(py);
        for event in events {
            state.add_run_diagnostic(unhandled_exception_diagnostic(
                py,
                &event,
                None,
                context.is_verbose(),
            ));
        }
        for error in errors {
            state.add_run_diagnostic(exception_hook_diagnostic("restore", &error.to_string()));
        }

        state.into_result()
    })
}
