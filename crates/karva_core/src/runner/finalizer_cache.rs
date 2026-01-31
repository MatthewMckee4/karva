use std::cell::RefCell;
use std::rc::Rc;

use pyo3::prelude::*;

use crate::Context;
use crate::extensions::fixtures::{Finalizer, FixtureScope};

/// Manages finalizers for fixtures at different scope levels.
#[derive(Debug, Default)]
pub struct FinalizerCache {
    session: Rc<RefCell<Vec<Finalizer>>>,

    package: Rc<RefCell<Vec<Finalizer>>>,

    module: Rc<RefCell<Vec<Finalizer>>>,

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
