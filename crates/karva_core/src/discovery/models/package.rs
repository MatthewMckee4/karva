use std::collections::HashMap;

use camino::Utf8PathBuf;

#[cfg(test)]
use crate::discovery::TestFunction;
use crate::{discovery::DiscoveredModule, name::ModulePath};

/// A package represents a single python directory.
#[derive(Debug)]
pub struct DiscoveredPackage {
    path: Utf8PathBuf,
    modules: HashMap<Utf8PathBuf, DiscoveredModule>,
    packages: HashMap<Utf8PathBuf, DiscoveredPackage>,
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

    #[cfg(test)]
    pub(crate) fn get_module(&self, path: &Utf8PathBuf) -> Option<&DiscoveredModule> {
        if let Some(module) = self.modules.get(path) {
            Some(module)
        } else {
            for subpackage in self.packages.values() {
                if let Some(found) = subpackage.get_module(path) {
                    return Some(found);
                }
            }
            None
        }
    }

    #[cfg(test)]
    pub(crate) fn get_package(&self, path: &Utf8PathBuf) -> Option<&Self> {
        if let Some(package) = self.packages.get(path) {
            Some(package)
        } else {
            for subpackage in self.packages.values() {
                if let Some(found) = subpackage.get_package(path) {
                    return Some(found);
                }
            }
            None
        }
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

    #[cfg(test)]
    pub(crate) fn total_test_functions(&self) -> usize {
        let mut total = 0;
        for module in self.modules.values() {
            total += module.total_test_functions();
        }
        for package in self.packages.values() {
            total += package.total_test_functions();
        }
        total
    }

    #[cfg(test)]
    pub(crate) fn test_functions(&self) -> Vec<&TestFunction> {
        let mut functions = Vec::new();
        for module in self.modules.values() {
            functions.extend(module.test_functions());
        }
        for package in self.packages.values() {
            functions.extend(package.test_functions());
        }
        functions
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

    #[cfg(test)]
    pub(crate) const fn display(&self) -> DisplayDiscoveredPackage<'_> {
        DisplayDiscoveredPackage::new(self)
    }
}

#[cfg(test)]
pub struct DisplayDiscoveredPackage<'proj> {
    package: &'proj DiscoveredPackage,
}

#[cfg(test)]
impl<'proj> DisplayDiscoveredPackage<'proj> {
    pub(crate) const fn new(package: &'proj DiscoveredPackage) -> Self {
        Self { package }
    }
}

#[cfg(test)]
impl std::fmt::Display for DisplayDiscoveredPackage<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fn write_tree(
            f: &mut std::fmt::Formatter<'_>,
            package: &DiscoveredPackage,
            prefix: &str,
        ) -> std::fmt::Result {
            let mut entries = Vec::new();

            let mut modules: Vec<_> = package.modules().values().collect();
            modules.sort_by_key(|m| m.name());

            for module in modules {
                let module_string = module.display().to_string();
                entries.push(("module", module_string));
            }

            let mut packages: Vec<_> = package.packages().iter().collect();
            packages.sort_by_key(|(name, _)| name.to_string());

            for (name, _) in &packages {
                let package_string = name.to_string();
                entries.push(("package", package_string));
            }

            let total = entries.len();
            for (i, (kind, display)) in entries.into_iter().enumerate() {
                let is_last_entry = i == total - 1;
                let branch = if is_last_entry {
                    "└── "
                } else {
                    "├── "
                };
                let child_prefix = if is_last_entry { "    " } else { "│   " };

                match kind {
                    "module" => {
                        let mut lines = display.lines();
                        if let Some(first_line) = lines.next() {
                            writeln!(f, "{prefix}{branch}{first_line}")?;
                        }
                        for line in lines {
                            writeln!(f, "{prefix}{child_prefix}{line}")?;
                        }
                    }
                    "package" => {
                        writeln!(f, "{prefix}{branch}{display}/")?;
                        let subpackage = &package.packages()[&Utf8PathBuf::from(display)];
                        write_tree(f, subpackage, &format!("{prefix}{child_prefix}"))?;
                    }
                    _ => {}
                }
            }
            Ok(())
        }

        write_tree(f, self.package, "")
    }
}
