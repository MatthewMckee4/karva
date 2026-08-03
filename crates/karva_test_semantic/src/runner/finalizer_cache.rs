//! Scope-aware storage and execution for fixture teardown callbacks.

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

    /// Drains one scope so callbacks can run without holding a runtime borrow.
    pub(super) fn take_scope(&mut self, scope: ScopeKey<'_>) -> Vec<Finalizer> {
        self.storage.take(scope)
    }

    /// Drains finalizers owned by one fixture while retaining unrelated scope state.
    pub(super) fn take_fixture(&mut self, fixture_name: &str) -> Vec<Finalizer> {
        let mut collected = Vec::new();
        self.storage.for_each_mut(|finalizers| {
            let (matching, remaining) = std::mem::take(finalizers)
                .into_iter()
                .partition(|finalizer| finalizer.fixture_name.as_deref() == Some(fixture_name));
            *finalizers = remaining;
            collected.extend(matching);
        });
        collected
    }
}
