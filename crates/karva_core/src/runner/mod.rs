use karva_project::{Db, ProjectDatabase};

use crate::{
    Context, DummyReporter, Reporter, TestRunResult, discovery::StandardDiscoverer,
    normalize::Normalizer, utils::attach_with_project,
};

mod finalizer_cache;
mod fixture_cache;
mod package_runner;

use finalizer_cache::FinalizerCache;
use fixture_cache::FixtureCache;
use package_runner::NormalizedPackageRunner;

pub trait TestRunner {
    fn test(&self) -> TestRunResult {
        self.test_with_reporter(&DummyReporter)
    }
    fn test_with_reporter(&self, reporter: &dyn Reporter) -> TestRunResult;
}

pub struct StandardTestRunner<'db> {
    db: &'db dyn Db,
}

impl<'db> StandardTestRunner<'db> {
    pub const fn new(db: &'db dyn Db) -> Self {
        Self { db }
    }

    fn test_impl(&self, reporter: &dyn Reporter) -> TestRunResult {
        attach_with_project(self.db, |py| {
            let context = Context::new(self.db, reporter);

            let session = StandardDiscoverer::new(&context).discover_with_py(py);

            let normalized_session = Normalizer::default().normalize(py, &session);

            NormalizedPackageRunner::new(&context).execute(py, normalized_session);

            context.into_result()
        })
    }
}

impl TestRunner for StandardTestRunner<'_> {
    fn test_with_reporter(&self, reporter: &dyn Reporter) -> TestRunResult {
        self.test_impl(reporter)
    }
}

impl TestRunner for ProjectDatabase {
    fn test_with_reporter(&self, reporter: &dyn Reporter) -> TestRunResult {
        let test_runner = StandardTestRunner::new(self);
        test_runner.test_with_reporter(reporter)
    }
}
