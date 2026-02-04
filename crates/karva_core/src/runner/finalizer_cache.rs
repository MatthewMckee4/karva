use std::cell::RefCell;
use std::rc::Rc;

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
    session: Rc<RefCell<Vec<Finalizer>>>,

    /// Package-scoped finalizers (run after each package).
    package: Rc<RefCell<Vec<Finalizer>>>,

    /// Module-scoped finalizers (run after each module).
    module: Rc<RefCell<Vec<Finalizer>>>,

    /// Function-scoped finalizers (run after each test).
    function: Rc<RefCell<Vec<Finalizer>>>,
}

impl FinalizerCache {
    pub fn add_finalizer(&self, finalizer: Finalizer) {
        match finalizer.scope {
            FixtureScope::Session => self.session.borrow_mut().push(finalizer),
            FixtureScope::Package => self.package.borrow_mut().push(finalizer),
            FixtureScope::Module => self.module.borrow_mut().push(finalizer),
            FixtureScope::Function => self.function.borrow_mut().push(finalizer),
        }
    }

    pub fn run_and_clear_scope(&self, context: &Context, py: Python<'_>, scope: FixtureScope) {
        let finalizers = match scope {
            FixtureScope::Session => {
                let mut guard = self.session.borrow_mut();
                guard.drain(..).collect::<Vec<_>>()
            }
            FixtureScope::Package => {
                let mut guard = self.package.borrow_mut();
                guard.drain(..).collect::<Vec<_>>()
            }
            FixtureScope::Module => {
                let mut guard = self.module.borrow_mut();
                guard.drain(..).collect::<Vec<_>>()
            }
            FixtureScope::Function => {
                let mut guard = self.function.borrow_mut();
                guard.drain(..).collect::<Vec<_>>()
            }
        };

        // Run finalizers in reverse order (LIFO)
        finalizers
            .into_iter()
            .rev()
            .for_each(|finalizer| finalizer.run(context, py));
    }
}
