//! Scoped ownership and setup order for fixture instances.

use std::collections::HashMap;
use std::rc::{Rc, Weak};

use crate::extensions::fixtures::FixtureIdentity;
use crate::runner::package_runner::FixtureInstance;
use crate::runner::scoped_storage::{ScopeKey, ScopedStorage};

#[derive(Default)]
struct ScopeFixtures {
    instances: HashMap<FixtureIdentity, Rc<FixtureInstance>>,
    setup_order: Vec<Weak<FixtureInstance>>,
}

/// Holds initialized fixtures in scope and preserves their teardown order.
#[derive(Default)]
pub(super) struct FixtureCache {
    storage: ScopedStorage<ScopeFixtures>,
}

impl FixtureCache {
    pub(super) fn has_values(&self, scope: ScopeKey<'_>) -> bool {
        self.storage
            .with(scope, |values| !values.instances.is_empty())
            .unwrap_or(false)
    }

    pub(super) fn get(
        &self,
        identity: &FixtureIdentity,
        scope: ScopeKey<'_>,
    ) -> Option<Rc<FixtureInstance>> {
        self.storage
            .with(scope, |values| values.instances.get(identity).cloned())
            .flatten()
    }

    pub(super) fn insert(&mut self, instance: &Rc<FixtureInstance>) {
        self.storage.with_mut(instance.scope_key(), |values| {
            values.setup_order.push(Rc::downgrade(instance));
            values
                .instances
                .insert(instance.identity().clone(), Rc::clone(instance));
        });
    }

    pub(super) fn remove(&mut self, instance: &FixtureInstance) {
        self.storage.with_mut(instance.scope_key(), |values| {
            values.instances.remove(instance.identity());
        });
    }

    /// Takes teardown order while keeping fixture values available to callbacks.
    pub(super) fn take_order(&mut self, scope: ScopeKey<'_>) -> Vec<Weak<FixtureInstance>> {
        self.storage
            .with_mut(scope, |values| std::mem::take(&mut values.setup_order))
    }

    pub(super) fn clear(&mut self, scope: ScopeKey<'_>) {
        self.storage.take(scope);
    }
}
