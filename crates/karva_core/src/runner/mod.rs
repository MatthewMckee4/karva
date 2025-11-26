use karva_project::Project;

use crate::{
    Context, DummyReporter, Reporter, discovery::StandardDiscoverer,
    normalize::DiscoveredPackageNormalizer, utils::attach_with_project,
};

pub mod diagnostic;
mod finalizer_cache;
mod fixture_cache;
mod package_runner;

pub use diagnostic::TestRunResult;
use finalizer_cache::FinalizerCache;
use fixture_cache::FixtureCache;
use package_runner::NormalizedPackageRunner;

pub trait TestRunner {
    fn test(&self) -> TestRunResult {
        self.test_with_reporter(&DummyReporter)
    }
    fn test_with_reporter(&self, reporter: &dyn Reporter) -> TestRunResult;
}

pub struct StandardTestRunner<'proj> {
    project: &'proj Project,
}

impl<'proj> StandardTestRunner<'proj> {
    pub const fn new(project: &'proj Project) -> Self {
        Self { project }
    }

    fn test_impl(&self, reporter: &dyn Reporter) -> TestRunResult {
        attach_with_project(self.project, |py| {
            let mut context = Context::new(self.project, reporter);

            let (session, discovery_diagnostics) =
                StandardDiscoverer::new(self.project).discover_with_py(py);

            context
                .result_mut()
                .add_discovery_diagnostics(discovery_diagnostics);

            // Normalize the discovered session - this resolves all parametrized fixtures
            // by splitting them into individual fixtures with new names matching the param.
            // Test functions are also expanded to match the new fixture names.
            let normalized_session = DiscoveredPackageNormalizer::new().normalize(py, &session);

            NormalizedPackageRunner::new(&mut context).run(py, &normalized_session);

            context.into_result()
        })
    }
}

impl TestRunner for StandardTestRunner<'_> {
    fn test_with_reporter(&self, reporter: &dyn Reporter) -> TestRunResult {
        self.test_impl(reporter)
    }
}

impl TestRunner for Project {
    fn test_with_reporter(&self, reporter: &dyn Reporter) -> TestRunResult {
        let test_runner = StandardTestRunner::new(self);
        test_runner.test_with_reporter(reporter)
    }
}

#[cfg(test)]
use karva_test::TestContext;

#[cfg(test)]
impl TestRunner for TestContext {
    fn test_with_reporter(&self, reporter: &dyn Reporter) -> TestRunResult {
        let project = Project::new(self.cwd(), vec![self.cwd()]);
        let test_runner = StandardTestRunner::new(&project);
        test_runner.test_with_reporter(reporter)
    }
}
