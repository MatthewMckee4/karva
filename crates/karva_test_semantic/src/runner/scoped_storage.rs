//! Shared storage primitive for state isolated by fixture lifetime.

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
    session: T,
    packages: HashMap<Utf8PathBuf, T>,
    module: T,
    function: T,
}

impl<T: Default> ScopedStorage<T> {
    /// Reads state when it exists. Package keys are absent until first use.
    pub(super) fn with<R>(&self, key: ScopeKey<'_>, read: impl FnOnce(&T) -> R) -> Option<R> {
        match key {
            ScopeKey::Session => Some(read(&self.session)),
            ScopeKey::Package(path) => self.packages.get(path).map(read),
            ScopeKey::Module => Some(read(&self.module)),
            ScopeKey::Function => Some(read(&self.function)),
        }
    }

    /// Mutates state, creating package-owned state on first use.
    pub(super) fn with_mut<R>(&mut self, key: ScopeKey<'_>, update: impl FnOnce(&mut T) -> R) -> R {
        match key {
            ScopeKey::Session => update(&mut self.session),
            ScopeKey::Package(path) => update(self.packages.entry(path.to_path_buf()).or_default()),
            ScopeKey::Module => update(&mut self.module),
            ScopeKey::Function => update(&mut self.function),
        }
    }

    /// Takes all state owned by a completed lifetime.
    pub(super) fn take(&mut self, key: ScopeKey<'_>) -> T {
        match key {
            ScopeKey::Session => std::mem::take(&mut self.session),
            ScopeKey::Package(path) => self.packages.remove(path).unwrap_or_default(),
            ScopeKey::Module => std::mem::take(&mut self.module),
            ScopeKey::Function => std::mem::take(&mut self.function),
        }
    }
}
