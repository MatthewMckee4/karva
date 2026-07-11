pub(crate) mod collection;
mod context;
pub(crate) mod diagnostic;
pub(crate) mod discovery;
pub(crate) mod extensions;
mod py_attach;
mod python;
mod runner;
pub mod utils;

pub(crate) use context::Context;
pub use karva_coverage::CoverageConfig;
pub use python::init_module;

use camino::Utf8Path;
use karva_coverage::CoverageSession;
use karva_diagnostic::{Reporter, TestRunResult};
use karva_metadata::ProjectSettings;
use karva_project::path::{TestPath, TestPathError};
use ruff_python_ast::PythonVersion;

use crate::discovery::StandardDiscoverer;
use crate::py_attach::attach_with_output;
use crate::runner::PackageRunner;

/// Run tests given the system, settings, Python version, reporter, and test paths.
///
/// This encapsulates the core test execution logic: attaching to a Python interpreter,
/// discovering tests, and running them.
pub fn run_tests(
    cwd: &Utf8Path,
    settings: &ProjectSettings,
    python_version: PythonVersion,
    reporter: &dyn Reporter,
    test_paths: Vec<Result<TestPath, TestPathError>>,
    coverage: Option<&CoverageConfig>,
) -> TestRunResult {
    let context = Context::new(cwd, settings, python_version, reporter);

    attach_with_output(settings.terminal().show_python_output, |py| {
        let cov_session = coverage.and_then(|cfg| match CoverageSession::start(py, cwd, cfg) {
            Ok(session) => Some(session),
            Err(err) => {
                tracing::error!("Failed to start coverage measurement: {err}");
                None
            }
        });

        let session = StandardDiscoverer::new(&context).discover_with_py(py, test_paths);

        PackageRunner::new(&context, cov_session.as_ref()).execute(py, &session);

        if let Some(cov_session) = cov_session
            && let Err(err) = cov_session.stop_and_save(py)
        {
            tracing::error!("Failed to save coverage data: {err}");
        }

        context.into_result()
    })
}
