use std::sync::{Arc, Mutex};

use pyo3::prelude::*;

use crate::{
    Context,
    extensions::fixtures::{Finalizer, FixtureScope},
};

/// Manages finalizers for fixtures at different scope levels.
#[derive(Debug, Default)]
pub struct FinalizerCache {
    session: Arc<Mutex<Vec<Finalizer>>>,

    package: Arc<Mutex<Vec<Finalizer>>>,

    module: Arc<Mutex<Vec<Finalizer>>>,

    function: Arc<Mutex<Vec<Finalizer>>>,
}

impl FinalizerCache {
    pub fn add_finalizer(&self, finalizer: Finalizer) {
        match finalizer.scope {
            FixtureScope::Session => self.session.lock().unwrap().push(finalizer),
            FixtureScope::Package => self.package.lock().unwrap().push(finalizer),
            FixtureScope::Module => self.module.lock().unwrap().push(finalizer),
            FixtureScope::Function => self.function.lock().unwrap().push(finalizer),
        }
    }

    pub fn run_and_clear_scope(&self, context: &Context, py: Python<'_>, scope: FixtureScope) {
        let finalizers = match scope {
            FixtureScope::Session => {
                let mut guard = self.session.lock().unwrap();
                guard.drain(..).collect::<Vec<_>>()
            }
            FixtureScope::Package => {
                let mut guard = self.package.lock().unwrap();
                guard.drain(..).collect::<Vec<_>>()
            }
            FixtureScope::Module => {
                let mut guard = self.module.lock().unwrap();
                guard.drain(..).collect::<Vec<_>>()
            }
            FixtureScope::Function => {
                let mut guard = self.function.lock().unwrap();
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
