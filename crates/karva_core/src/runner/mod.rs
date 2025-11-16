use karva_project::Project;

use crate::{
    DummyReporter, Reporter, collection::TestCaseCollector, discovery::StandardDiscoverer,
    utils::attach,
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
            let mut run_result = TestRunResult::default();

            let (session, discovery_diagnostics) =
                StandardDiscoverer::new(self.project).discover(py);

            run_result.add_discovery_diagnostics(discovery_diagnostics);

            let collected_session = TestCaseCollector::collect(py, &session);

            let total_test_cases = collected_session.total_test_cases();

            reporter.log_test_count(total_test_cases);

            collected_session.run(py, self.project, reporter, &mut run_result);

            run_result
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
