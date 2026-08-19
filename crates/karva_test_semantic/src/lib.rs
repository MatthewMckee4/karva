//! Python test semantics, fixture resolution, extensions, and in-process execution.

use std::collections::BTreeSet;

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
mod utils;

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

use crate::diagnostic::failed_to_start_coverage_diagnostic;
use crate::discovery::{DiscoveryIssue, StandardDiscoverer};
use crate::py_attach::attach_with_output;
use crate::runner::PackageRunner;

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

        PackageRunner::new(&context, &mut state, cov_session.as_ref())
            .execute(py, &discovery.package);

        if let Some(cov_session) = cov_session
            && let Err(err) = cov_session.stop_and_save(py)
        {
            tracing::error!("Failed to save coverage data: {err}");
        }

        state.into_result()
    })
}
