pub(crate) mod collection;
mod context;
pub(crate) mod diagnostic;
pub(crate) mod discovery;
pub(crate) mod extensions;
mod python;
mod runner;
pub mod utils;

pub(crate) use context::Context;
pub use python::init_module;

use camino::Utf8Path;
use karva_diagnostic::{Reporter, TestRunResult};
use karva_metadata::ProjectSettings;
use karva_project::path::{TestPath, TestPathError};
use ruff_python_ast::PythonVersion;

use crate::discovery::StandardDiscoverer;
use crate::runner::PackageRunner;
use crate::utils::attach_with_project;

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
) -> TestRunResult {
    let context = Context::new(cwd, settings, python_version, reporter);

    attach_with_project(settings.terminal().show_python_output, |py| {
        let session = StandardDiscoverer::new(&context).discover_with_py(py, test_paths);

        PackageRunner::new(&context).execute(py, &session);

        context.into_result()
    })
}
