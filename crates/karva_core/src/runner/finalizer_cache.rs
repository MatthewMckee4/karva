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
    pub fn new() -> Self {
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

    /// Run and clear finalizers at a specific scope level
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

    /// Run and clear function-scoped finalizers
    pub fn run_and_clear_function(&mut self, py: Python<'_>) -> Vec<Diagnostic> {
        self.run_and_clear_scope(py, FixtureScope::Function)
    }

    /// Run and clear module-scoped finalizers
    pub fn run_and_clear_module(&mut self, py: Python<'_>) -> Vec<Diagnostic> {
        self.run_and_clear_scope(py, FixtureScope::Module)
    }

    /// Run and clear package-scoped finalizers
    pub fn run_and_clear_package(&mut self, py: Python<'_>) -> Vec<Diagnostic> {
        self.run_and_clear_scope(py, FixtureScope::Package)
    }

    /// Run and clear session-scoped finalizers
    pub fn run_and_clear_session(&mut self, py: Python<'_>) -> Vec<Diagnostic> {
        self.run_and_clear_scope(py, FixtureScope::Session)
    }

    /// Check if there are any finalizers at a specific scope
    pub fn has_finalizers(&self, scope: FixtureScope) -> bool {
        match scope {
            FixtureScope::Session => !self.session.is_empty(),
            FixtureScope::Package => !self.package.is_empty(),
            FixtureScope::Module => !self.module.is_empty(),
            FixtureScope::Function => !self.function.is_empty(),
        }
    }
}

impl Default for FinalizerCache {
    fn default() -> Self {
        Self::new()
    }
}
