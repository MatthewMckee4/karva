use std::collections::HashMap;

use camino::Utf8PathBuf;

use crate::{discovery::DiscoveredModule, name::ModulePath};

/// A package represents a single python directory.
#[derive(Debug)]
pub struct DiscoveredPackage {
    path: Utf8PathBuf,
    modules: HashMap<Utf8PathBuf, DiscoveredModule>,
    packages: HashMap<Utf8PathBuf, Self>,
    configuration_module_path: Option<ModulePath>,
}

impl DiscoveredPackage {
    pub(crate) fn new(path: Utf8PathBuf) -> Self {
        Self {
            path,
            modules: HashMap::new(),
            packages: HashMap::new(),
            configuration_module_path: None,
        }
    }

    pub(crate) const fn path(&self) -> &Utf8PathBuf {
        &self.path
    }

    pub(crate) const fn modules(&self) -> &HashMap<Utf8PathBuf, DiscoveredModule> {
        &self.modules
    }

    pub(crate) const fn packages(&self) -> &HashMap<Utf8PathBuf, Self> {
        &self.packages
    }

    /// Add a module directly to this package.
    pub(crate) fn add_direct_module(&mut self, module: DiscoveredModule) {
        self.modules.insert(module.path().clone(), module);
    }

    pub(crate) fn add_configuration_module(&mut self, module: DiscoveredModule) {
        self.configuration_module_path = Some(module.module_path().clone());
        self.add_direct_module(module);
    }

    /// Adds a package directly as a subpackage.
    pub(crate) fn add_direct_subpackage(&mut self, other: Self) {
        self.packages.insert(other.path().clone(), other);
    }

    pub(crate) fn configuration_module_impl(&self) -> Option<&DiscoveredModule> {
        self.configuration_module_path.as_ref().map(|module_path| {
            self.modules
                .get(module_path.path())
                .expect("If configuration module path is not none, we should be able to find it")
        })
    }

    /// Remove empty modules and packages.
    pub(crate) fn shrink(&mut self) {
        self.modules.retain(|_, module| !module.is_empty());

        if let Some(configuration_module) = self.configuration_module_path.as_ref() {
            if !self.modules.contains_key(configuration_module.path()) {
                self.configuration_module_path = None;
            }
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
