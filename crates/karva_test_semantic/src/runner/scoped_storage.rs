//! Shared storage primitive for state isolated by fixture scope.

use std::cell::RefCell;

use crate::extensions::fixtures::FixtureScope;

/// A per-scope storage container that maps each `FixtureScope` to its own `RefCell<T>`.
///
/// Used by both `FixtureCache` and `FinalizerCache` to avoid duplicating the same
/// four-field struct and `match`-based accessor.
#[derive(Debug, Default)]
pub(super) struct ScopedStorage<T: Default> {
    /// Values retained for the complete worker session.
    session: RefCell<T>,
    /// Values retained while one discovered package executes.
    package: RefCell<T>,
    /// Values retained while one test module executes.
    module: RefCell<T>,
    /// Values retained for one concrete test attempt.
    function: RefCell<T>,
}

impl<T: Default> ScopedStorage<T> {
    /// Returns interior-mutable storage belonging to `scope`.
    pub(super) fn get(&self, scope: FixtureScope) -> &RefCell<T> {
        match scope {
            FixtureScope::Session => &self.session,
            FixtureScope::Package => &self.package,
            FixtureScope::Module => &self.module,
            FixtureScope::Function => &self.function,
        }
    }
}
