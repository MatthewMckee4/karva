use pyo3::prelude::*;

use crate::{
    diagnostic::Diagnostic,
    extensions::fixtures::{Finalizer, FixtureScope},
};

/// Manages finalizers (cleanup functions) for fixtures at different scope levels.
///
/// Finalizers are stored at the scope level where they were created and run when that scope ends:
/// - Session: Run at the end of the entire test session
/// - Package: Run at the end of each package
/// - Module: Run at the end of each module
/// - Function: Run at the end of each test function
pub struct FinalizerCache {
    /// Session-scoped finalizers
    session: Vec<Finalizer>,

    /// Package-scoped finalizers
    package: Vec<Finalizer>,

    /// Module-scoped finalizers
    module: Vec<Finalizer>,

    /// Function-scoped finalizers
    function: Vec<Finalizer>,
}

impl FinalizerCache {
    pub const fn new() -> Self {
        Self {
            session: Vec::new(),
            package: Vec::new(),
            module: Vec::new(),
            function: Vec::new(),
        }
    }

    /// Add a finalizer at the appropriate scope level
    pub fn add_finalizer(&mut self, finalizer: Finalizer) {
        match finalizer.scope() {
            FixtureScope::Session => self.session.push(finalizer),
            FixtureScope::Package => self.package.push(finalizer),
            FixtureScope::Module => self.module.push(finalizer),
            FixtureScope::Function => self.function.push(finalizer),
        }
    }

    /// Run and clear finalizers athist a specific scope level
    pub fn run_and_clear_scope(&mut self, py: Python<'_>, scope: FixtureScope) -> Vec<Diagnostic> {
        let finalizers = match scope {
            FixtureScope::Session => std::mem::take(&mut self.session),
            FixtureScope::Package => std::mem::take(&mut self.package),
            FixtureScope::Module => std::mem::take(&mut self.module),
            FixtureScope::Function => std::mem::take(&mut self.function),
        };

        // Run finalizers in reverse order (LIFO - last in, first out)
        let mut diagnostics = Vec::new();
        for finalizer in finalizers.into_iter().rev() {
            if let Some(diagnostic) = finalizer.run(py) {
                diagnostics.push(diagnostic);
            }
        }

        diagnostics
    }
}

impl Default for FinalizerCache {
    fn default() -> Self {
        Self::new()
    }
}
