use std::cell::RefCell;

use pyo3::prelude::*;

use crate::Context;
use crate::extensions::fixtures::{Finalizer, FixtureScope};

/// Manages fixture teardown callbacks at different scope levels.
///
/// Finalizers are collected during fixture setup and executed in LIFO
/// order when their scope ends (e.g., after a test, module, or package).
#[derive(Debug, Default)]
pub struct FinalizerCache {
    /// Session-scoped finalizers (run at end of test run).
    session: RefCell<Vec<Finalizer>>,

    /// Package-scoped finalizers (run after each package).
    package: RefCell<Vec<Finalizer>>,

    /// Module-scoped finalizers (run after each module).
    module: RefCell<Vec<Finalizer>>,

    /// Function-scoped finalizers (run after each test).
    function: RefCell<Vec<Finalizer>>,
}

impl FinalizerCache {
    fn scope_storage(&self, scope: FixtureScope) -> &RefCell<Vec<Finalizer>> {
        match scope {
            FixtureScope::Session => &self.session,
            FixtureScope::Package => &self.package,
            FixtureScope::Module => &self.module,
            FixtureScope::Function => &self.function,
        }
    }

    pub(crate) fn add_finalizer(&self, finalizer: Finalizer) {
        self.scope_storage(finalizer.scope)
            .borrow_mut()
            .push(finalizer);
    }

    pub(crate) fn run_and_clear_scope(
        &self,
        context: &Context,
        py: Python<'_>,
        scope: FixtureScope,
    ) {
        let finalizers: Vec<_> = self.scope_storage(scope).borrow_mut().drain(..).collect();

        // Run finalizers in reverse order (LIFO)
        finalizers
            .into_iter()
            .rev()
            .for_each(|finalizer| finalizer.run(context, py));
    }
}
