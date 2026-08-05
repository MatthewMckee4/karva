//! Scope-aware storage and execution for fixture teardown callbacks.

use karva_diagnostic::Diagnostic;
use pyo3::prelude::*;

use crate::extensions::fixtures::{Finalizer, FixtureScope};
use crate::runner::scoped_storage::{ScopeKey, ScopedStorage};

/// Manages fixture teardown callbacks at different scope levels.
///
/// Finalizers are collected during fixture setup and executed in LIFO
/// order when their scope ends (e.g., after a test, module, or package).
#[derive(Debug, Default)]
pub(super) struct FinalizerCache {
    /// Finalizer stacks isolated by fixture scope.
    storage: ScopedStorage<Vec<Finalizer>>,
}

impl FinalizerCache {
    /// Adds a finalizer to its declared scope's LIFO stack.
    pub(super) fn add_finalizer(&mut self, finalizer: Finalizer) {
        let package_owner = finalizer.package_owner.clone();
        let scope = match finalizer.scope {
            FixtureScope::Session => ScopeKey::Session,
            FixtureScope::Package => ScopeKey::Package(&package_owner),
            FixtureScope::Module => ScopeKey::Module,
            FixtureScope::Function => ScopeKey::Function,
        };
        self.storage.with_mut(scope, |finalizers| {
            finalizers.push(finalizer);
        });
    }

    /// Drains one scope and returns diagnostics raised during teardown.
    pub(super) fn run_and_clear_scope(
        &mut self,
        py: Python<'_>,
        scope: ScopeKey<'_>,
    ) -> Vec<Diagnostic> {
        self.storage
            .take(scope)
            .into_iter()
            .rev()
            .filter_map(|finalizer| finalizer.run(py).err())
            .collect()
    }
}
