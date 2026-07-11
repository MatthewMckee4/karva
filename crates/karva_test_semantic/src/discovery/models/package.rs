use std::collections::BTreeMap;

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
    modules: BTreeMap<Utf8PathBuf, DiscoveredModule>,

    /// Sub-packages within this package, keyed by directory path.
    packages: BTreeMap<Utf8PathBuf, Self>,

    /// Optional conftest.py module containing shared fixtures.
    configuration_module: Option<DiscoveredModule>,

    /// Optional synthetic module holding framework-provided fixtures (from
    /// `karva._builtins`). Only populated on the session root.
    framework_module: Option<DiscoveredModule>,
}

impl DiscoveredPackage {
    pub(crate) fn new(path: Utf8PathBuf) -> Self {
        Self {
            path,
            modules: BTreeMap::new(),
            packages: BTreeMap::new(),
            configuration_module: None,
            framework_module: None,
        }
    }

    pub(crate) fn path(&self) -> &Utf8PathBuf {
        &self.path
    }

    pub(crate) fn modules(&self) -> &BTreeMap<Utf8PathBuf, DiscoveredModule> {
        &self.modules
    }

    pub(crate) fn packages(&self) -> &BTreeMap<Utf8PathBuf, Self> {
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

    pub(crate) fn set_framework_module(&mut self, module: Option<DiscoveredModule>) {
        self.framework_module = module;
    }

    pub(crate) fn framework_module_impl(&self) -> Option<&DiscoveredModule> {
        self.framework_module.as_ref()
    }

    /// Remove empty modules and packages.
    pub(crate) fn shrink(&mut self) {
        for module in self.modules.values_mut() {
            module.shrink();
        }

        for package in self.packages.values_mut() {
            package.shrink();
        }

        self.modules.retain(|_, module| !module.is_empty());
        self.packages.retain(|_, package| !package.is_empty());
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.modules.is_empty() && self.packages.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use karva_python_semantic::ModulePath;

    use crate::discovery::DiscoveredModule;

    use super::DiscoveredPackage;

    fn module(path: &str) -> DiscoveredModule {
        DiscoveredModule::new_with_source(
            ModulePath::new_with_name(path, path.trim_end_matches(".py").to_string()),
            String::new(),
        )
    }

    #[test]
    fn shrink_removes_packages_that_become_empty_after_child_shrink() {
        let mut root = DiscoveredPackage::new(Utf8PathBuf::from("/project"));
        let mut child = DiscoveredPackage::new(Utf8PathBuf::from("/project/pkg"));
        let empty_path = Utf8PathBuf::from("/project/pkg/empty");
        child.add_direct_subpackage(DiscoveredPackage::new(empty_path));
        root.add_direct_subpackage(child);

        root.shrink();

        assert!(root.packages().is_empty());
    }

    #[test]
    fn modules_iterate_by_path() {
        let mut root = DiscoveredPackage::new(Utf8PathBuf::from("/project"));
        root.add_direct_module(module("/project/test_z.py"));
        root.add_direct_module(module("/project/test_a.py"));
        root.add_direct_module(module("/project/test_m.py"));

        let paths: Vec<_> = root.modules().keys().map(|path| path.as_str()).collect();

        assert_eq!(
            paths,
            [
                "/project/test_a.py",
                "/project/test_m.py",
                "/project/test_z.py"
            ]
        );
    }

    #[test]
    fn packages_iterate_by_path() {
        let mut root = DiscoveredPackage::new(Utf8PathBuf::from("/project"));
        root.add_direct_subpackage(DiscoveredPackage::new(Utf8PathBuf::from("/project/z")));
        root.add_direct_subpackage(DiscoveredPackage::new(Utf8PathBuf::from("/project/a")));
        root.add_direct_subpackage(DiscoveredPackage::new(Utf8PathBuf::from("/project/m")));

        let paths: Vec<_> = root.packages().keys().map(|path| path.as_str()).collect();

        assert_eq!(paths, ["/project/a", "/project/m", "/project/z"]);
    }
}
