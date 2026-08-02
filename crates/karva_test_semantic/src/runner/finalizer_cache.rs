//! Scope-aware storage and execution for fixture teardown callbacks.

use pyo3::prelude::*;
use ruff_db::diagnostic::Diagnostic;

use crate::extensions::fixtures::{Finalizer, FixtureScope};
use crate::runner::scoped_storage::ScopedStorage;

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
    pub(super) fn add_finalizer(&self, finalizer: Finalizer) {
        self.storage
            .get(finalizer.scope)
            .borrow_mut()
            .push(finalizer);
    }

    /// Drains one scope and returns diagnostics raised during teardown.
    pub(super) fn run_and_clear_scope(
        &self,
        py: Python<'_>,
        scope: FixtureScope,
    ) -> Vec<Diagnostic> {
        self.storage
            .get(scope)
            .borrow_mut()
            .drain(..)
            .rev()
            .filter_map(|finalizer| finalizer.run(py).err())
            .collect()
    }
}
