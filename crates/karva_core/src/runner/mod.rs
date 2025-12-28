use karva_diagnostic::{Reporter, TestRunResult};
use karva_project::Db;

use crate::Context;
use crate::discovery::StandardDiscoverer;
use crate::normalize::Normalizer;
use crate::utils::attach_with_project;

mod finalizer_cache;
mod fixture_cache;
mod package_runner;

use finalizer_cache::FinalizerCache;
use fixture_cache::FixtureCache;
use package_runner::NormalizedPackageRunner;

pub use package_runner::FixtureCallError;

pub struct StandardTestRunner<'db> {
    db: &'db dyn Db,
}

impl<'db> StandardTestRunner<'db> {
    pub const fn new(db: &'db dyn Db) -> Self {
        Self { db }
    }

    pub(crate) fn test_with_reporter(&self, reporter: &dyn Reporter) -> TestRunResult {
        attach_with_project(self.db.project().settings(), |py| {
            let context = Context::new(self.db, reporter);

            let session = StandardDiscoverer::new(&context).discover_with_py(py);

            let normalized_session = Normalizer::default().normalize(py, &session);

            NormalizedPackageRunner::new(&context).execute(py, normalized_session);

            context.into_result()
        })
    }
}
