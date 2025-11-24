use pyo3::prelude::*;

use crate::{
    diagnostic::Diagnostic,
    extensions::fixtures::{Finalizer, FixtureScope},
};

/// Manages finalizers for fixtures at different scope levels.
#[derive(Debug, Default)]
pub struct FinalizerCache {
    session: Vec<Finalizer>,

    package: Vec<Finalizer>,

    module: Vec<Finalizer>,

    function: Vec<Finalizer>,
}

impl FinalizerCache {
    pub fn add_finalizer(&mut self, finalizer: Finalizer) {
        match finalizer.scope() {
            FixtureScope::Session => self.session.push(finalizer),
            FixtureScope::Package => self.package.push(finalizer),
            FixtureScope::Module => self.module.push(finalizer),
            FixtureScope::Function => self.function.push(finalizer),
        }
    }

    pub fn run_and_clear_scope(&mut self, py: Python<'_>, scope: FixtureScope) -> Vec<Diagnostic> {
        let finalizers = match scope {
            FixtureScope::Session => std::mem::take(&mut self.session),
            FixtureScope::Package => std::mem::take(&mut self.package),
            FixtureScope::Module => std::mem::take(&mut self.module),
            FixtureScope::Function => std::mem::take(&mut self.function),
        };

        // Run finalizers in reverse order (LIFO)
        finalizers
            .into_iter()
            .rev()
            .filter_map(|finalizer| finalizer.run(py))
            .collect()
    }
}
