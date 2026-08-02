//! Shared storage primitive for state isolated by fixture lifetime.

use std::cell::RefCell;
use std::collections::HashMap;

use camino::{Utf8Path, Utf8PathBuf};

/// Exact runtime lifetime owning a fixture value or finalizer.
#[derive(Clone, Copy, Debug)]
pub(super) enum ScopeKey<'a> {
    Session,
    Package(&'a Utf8Path),
    Module,
    Function,
}

/// Storage for session, package, module, and function fixture state.
///
/// Package state is keyed by defining package. Nested package cleanup therefore
/// cannot clear values owned by an ancestor package.
#[derive(Debug, Default)]
pub(super) struct ScopedStorage<T: Default> {
    session: RefCell<T>,
    packages: RefCell<HashMap<Utf8PathBuf, T>>,
    module: RefCell<T>,
    function: RefCell<T>,
}

impl<T: Default> ScopedStorage<T> {
    /// Reads state when it exists. Package keys are absent until first use.
    pub(super) fn with<R>(&self, key: ScopeKey<'_>, read: impl FnOnce(&T) -> R) -> Option<R> {
        match key {
            ScopeKey::Session => Some(read(&self.session.borrow())),
            ScopeKey::Package(path) => self.packages.borrow().get(path).map(read),
            ScopeKey::Module => Some(read(&self.module.borrow())),
            ScopeKey::Function => Some(read(&self.function.borrow())),
        }
    }

    /// Mutates state, creating package-owned state on first use.
    pub(super) fn with_mut<R>(&self, key: ScopeKey<'_>, update: impl FnOnce(&mut T) -> R) -> R {
        match key {
            ScopeKey::Session => update(&mut self.session.borrow_mut()),
            ScopeKey::Package(path) => update(
                self.packages
                    .borrow_mut()
                    .entry(path.to_path_buf())
                    .or_default(),
            ),
            ScopeKey::Module => update(&mut self.module.borrow_mut()),
            ScopeKey::Function => update(&mut self.function.borrow_mut()),
        }
    }

    /// Takes all state owned by a completed lifetime.
    pub(super) fn take(&self, key: ScopeKey<'_>) -> T {
        match key {
            ScopeKey::Session => std::mem::take(&mut self.session.borrow_mut()),
            ScopeKey::Package(path) => self.packages.borrow_mut().remove(path).unwrap_or_default(),
            ScopeKey::Module => std::mem::take(&mut self.module.borrow_mut()),
            ScopeKey::Function => std::mem::take(&mut self.function.borrow_mut()),
        }
    }
}
