//! Python test semantics, fixture resolution, extensions, and in-process execution.

pub(crate) mod collection;
mod context;
pub(crate) mod diagnostic;
pub(crate) mod discovery;
pub(crate) mod extensions;
mod output_capture;
mod py_attach;
mod python;
mod runner;
pub mod utils;

pub(crate) use context::{Context, RunState};
pub use karva_coverage::CoverageConfig;
pub use python::init_module;

use camino::Utf8Path;
use karva_coverage::CoverageSession;
use karva_diagnostic::{Reporter, TestRunResult};
use karva_metadata::ProjectSettings;
use karva_project::path::{TestPath, TestPathError};
use ruff_python_ast::PythonVersion;

use crate::diagnostic::failed_to_start_coverage_diagnostic;
use crate::discovery::{DiscoveryIssue, StandardDiscoverer};
use crate::py_attach::attach_with_output;
use crate::runner::PackageRunner;

/// Runs discovery and execution inside one interpreter attachment.
///
/// Coverage startup or persistence failures are logged but do not discard test results.
pub fn run_tests(
    cwd: &Utf8Path,
    settings: &ProjectSettings,
    python_version: PythonVersion,
    reporter: &dyn Reporter,
    test_paths: Vec<Result<TestPath, TestPathError>>,
    coverage: Option<&CoverageConfig>,
    verbose: bool,
) -> TestRunResult {
    let context = Context::new(cwd, settings, python_version, reporter, verbose);
    let mut state = RunState::default();

    attach_with_output(settings.terminal().show_python_output, |py| {
        let cov_session = coverage.and_then(|cfg| match CoverageSession::start(py, cwd, cfg) {
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
                } => state.register_module_skip(&context, &module_path, reason),
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
