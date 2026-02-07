use std::collections::HashMap;

use camino::Utf8PathBuf;

use crate::discovery::DiscoveredModule;

/// Represents a Python package directory containing modules and sub-packages.
///
/// Organizes discovered modules hierarchically and holds any conftest.py
/// configuration module for fixture sharing.
#[derive(Debug)]
pub struct DiscoveredPackage {
    /// Filesystem path to this package directory.
    path: Utf8PathBuf,

    /// Test modules directly in this package, keyed by file path.
    modules: HashMap<Utf8PathBuf, DiscoveredModule>,

    /// Sub-packages within this package, keyed by directory path.
    packages: HashMap<Utf8PathBuf, Self>,

    /// Optional conftest.py module containing shared fixtures.
    configuration_module: Option<DiscoveredModule>,
}

impl DiscoveredPackage {
    pub(crate) fn new(path: Utf8PathBuf) -> Self {
        Self {
            path,
            modules: HashMap::new(),
            packages: HashMap::new(),
            configuration_module: None,
        }
    }

    pub(crate) const fn path(&self) -> &Utf8PathBuf {
        &self.path
    }

    pub(crate) fn modules(&self) -> &HashMap<Utf8PathBuf, DiscoveredModule> {
        &self.modules
    }

    pub(crate) const fn packages(&self) -> &HashMap<Utf8PathBuf, Self> {
        &self.packages
    }

    /// Add a module directly to this package.
    pub(crate) fn add_direct_module(&mut self, module: DiscoveredModule) {
        self.modules.insert(module.path().clone(), module);
    }

    pub(crate) fn set_configuration_module(&mut self, module: Option<DiscoveredModule>) {
        self.configuration_module = module;
    }

    /// Adds a package directly as a subpackage.
    pub(crate) fn add_direct_subpackage(&mut self, other: Self) {
        self.packages.insert(other.path().clone(), other);
    }

    pub(crate) fn configuration_module_impl(&self) -> Option<&DiscoveredModule> {
        self.configuration_module.as_ref()
    }

    /// Remove empty modules and packages.
    pub(crate) fn shrink(&mut self) {
        self.modules.retain(|_, module| !module.is_empty());

        for module in self.modules.values_mut() {
            module.shrink();
        }

        self.packages.retain(|_, package| !package.is_empty());

        for package in self.packages.values_mut() {
            package.shrink();
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.modules.is_empty() && self.packages.is_empty()
    }
}
