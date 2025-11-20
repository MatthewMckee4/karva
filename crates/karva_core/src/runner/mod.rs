use karva_project::Project;

use crate::{
    Context, DummyReporter, Reporter, collection::DiscoveredPackageRunner,
    discovery::StandardDiscoverer, utils::attach,
};

pub mod diagnostic;

pub use diagnostic::TestRunResult;

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
        attach(self.project, |py| {
            let mut context = Context::new(self.project, reporter);

            let (session, discovery_diagnostics) =
                StandardDiscoverer::new(self.project).discover(py);

            context
                .result_mut()
                .add_discovery_diagnostics(discovery_diagnostics);

            DiscoveredPackageRunner::new(&mut context).run(py, &session);

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
