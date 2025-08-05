use std::collections::{HashMap, HashSet};

use karva_project::path::SystemPathBuf;
use pyo3::prelude::*;

#[cfg(test)]
use crate::discovery::TestFunction;
use crate::{
    discovery::{DiscoveredModule, ModuleType},
    extensions::fixtures::{Fixture, HasFixtures, UsesFixtures},
};

/// A package represents a single python directory.
#[derive(Debug)]
pub(crate) struct DiscoveredPackage {
    path: SystemPathBuf,
    modules: HashMap<SystemPathBuf, DiscoveredModule>,
    packages: HashMap<SystemPathBuf, DiscoveredPackage>,
    configuration_modules: HashSet<SystemPathBuf>,
}

impl DiscoveredPackage {
    #[must_use]
    pub(crate) fn new(path: SystemPathBuf) -> Self {
        Self {
            path,
            modules: HashMap::new(),
            packages: HashMap::new(),
            configuration_modules: HashSet::new(),
        }
    }

    #[must_use]
    pub(crate) const fn path(&self) -> &SystemPathBuf {
        &self.path
    }

    #[must_use]
    pub(crate) const fn modules(&self) -> &HashMap<SystemPathBuf, DiscoveredModule> {
        &self.modules
    }

    #[must_use]
    pub(crate) const fn packages(&self) -> &HashMap<SystemPathBuf, Self> {
        &self.packages
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn get_module(&self, path: &SystemPathBuf) -> Option<&DiscoveredModule> {
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

    #[must_use]
    #[cfg(test)]
    pub(crate) fn get_package(&self, path: &SystemPathBuf) -> Option<&Self> {
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

    /// Add a module to this package.
    ///
    /// If the module path does not start with our path, do nothing.
    ///
    /// If the module path equals our path, use update method.
    ///
    /// Otherwise, strip the current path from the start and add the module to the appropriate sub-package.
    pub(crate) fn add_module(&mut self, module: DiscoveredModule) {
        if !module.path().starts_with(self.path()) {
            return;
        }

        if module.path().parent().is_some_and(|p| p == self.path()) {
            if let Some(existing_module) = self.modules.get_mut(module.path()) {
                existing_module.update(module);
            } else {
                if module.module_type() == ModuleType::Configuration {
                    self.configuration_modules.insert(module.path().clone());
                }
                self.modules.insert(module.path().clone(), module);
            }
            return;
        }

        let Ok(relative_path) = module.path().strip_prefix(self.path()) else {
            return;
        };

        let components: Vec<_> = relative_path.components().collect();

        if components.is_empty() {
            return;
        }

        let first_component = components[0];
        let intermediate_path = self.path().join(first_component);

        if let Some(existing_package) = self.packages.get_mut(&intermediate_path) {
            existing_package.add_module(module);
        } else {
            let mut new_package = Self::new(intermediate_path);
            new_package.add_module(module);
            self.packages
                .insert(new_package.path().clone(), new_package);
        }
    }

    pub(crate) fn add_configuration_module(&mut self, module: DiscoveredModule) {
        self.configuration_modules.insert(module.path().clone());
        self.add_module(module);
    }

    /// Add a package to this package.
    ///
    /// If the package path equals our path, use update method.
    ///
    /// Otherwise, strip the current path from the start and add the package to the appropriate sub-package.
    pub(crate) fn add_package(&mut self, package: Self) {
        if !package.path().starts_with(self.path()) {
            return;
        }

        if package.path() == self.path() {
            self.update(package);
            return;
        }

        let Ok(relative_path) = package.path().strip_prefix(self.path()) else {
            return;
        };

        let components: Vec<_> = relative_path.components().collect();

        if components.is_empty() {
            return;
        }

        let first_component = components[0];
        let intermediate_path = self.path().join(first_component);

        if let Some(existing_package) = self.packages.get_mut(&intermediate_path) {
            existing_package.add_package(package);
        } else {
            let mut new_package = Self::new(intermediate_path);
            new_package.add_package(package);
            self.packages
                .insert(new_package.path().clone(), new_package);
        }
    }

    #[must_use]
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

    pub(crate) fn update(&mut self, package: Self) {
        for (_, module) in package.modules {
            self.add_module(module);
        }
        for (_, package) in package.packages {
            self.add_package(package);
        }

        for module in package.configuration_modules {
            self.configuration_modules.insert(module);
        }
    }

    #[must_use]
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

    /// Get all the test functions and fixtures that are used in this package.
    #[must_use]
    pub(crate) fn all_uses_fixtures(&self) -> Vec<&dyn UsesFixtures> {
        let mut dependencies: Vec<&dyn UsesFixtures> = Vec::new();

        for module in self.modules.values() {
            dependencies.extend(module.all_uses_fixtures());
        }

        for package in self.packages.values() {
            dependencies.extend(package.all_uses_fixtures());
        }

        dependencies
    }

    #[must_use]
    pub(crate) fn configuration_modules(&self) -> Vec<&DiscoveredModule> {
        self.configuration_modules
            .iter()
            .filter_map(|path| self.modules.get(path))
            .collect()
    }

    /// Remove empty modules and packages.
    pub(crate) fn shrink(&mut self) {
        self.modules.retain(|path, module| {
            if module.is_empty() {
                self.configuration_modules.remove(path);
                false
            } else {
                true
            }
        });

        self.packages.retain(|_, package| !package.is_empty());

        for package in self.packages.values_mut() {
            package.shrink();
        }
    }

    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.modules.is_empty() && self.packages.is_empty()
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) const fn display(&self) -> DisplayDiscoveredPackage<'_> {
        DisplayDiscoveredPackage::new(self)
    }
}

impl<'proj> HasFixtures<'proj> for DiscoveredPackage {
    fn all_fixtures<'a: 'proj>(
        &'a self,
        py: Python<'_>,
        test_cases: &[&dyn UsesFixtures],
    ) -> Vec<&'proj Fixture> {
        let mut fixtures = Vec::new();

        for module in self.configuration_modules() {
            let module_fixtures = module.all_fixtures(py, test_cases);

            fixtures.extend(module_fixtures);
        }

        fixtures
    }
}

impl<'proj> HasFixtures<'proj> for &'proj DiscoveredPackage {
    fn all_fixtures<'a: 'proj>(
        &'a self,
        py: Python<'_>,
        test_cases: &[&dyn UsesFixtures],
    ) -> Vec<&'proj Fixture> {
        (*self).all_fixtures(py, test_cases)
    }
}

#[cfg(test)]
pub(crate) struct DisplayDiscoveredPackage<'proj> {
    package: &'proj DiscoveredPackage,
}

#[cfg(test)]
impl<'proj> DisplayDiscoveredPackage<'proj> {
    #[must_use]
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
                entries.push(("module", module.display().to_string()));
            }

            let mut packages: Vec<_> = package.packages().iter().collect();
            packages.sort_by_key(|(name, _)| name.display().to_string());

            for (name, _) in &packages {
                entries.push(("package", name.display().to_string()));
            }

            let total = entries.len();
            for (i, (kind, name)) in entries.into_iter().enumerate() {
                let is_last_entry = i == total - 1;
                let branch = if is_last_entry {
                    "└── "
                } else {
                    "├── "
                };
                let child_prefix = if is_last_entry { "    " } else { "│   " };

                match kind {
                    "module" => {
                        let mut lines = name.lines();
                        if let Some(first_line) = lines.next() {
                            writeln!(f, "{prefix}{branch}{first_line}")?;
                        }
                        for line in lines {
                            writeln!(f, "{prefix}{child_prefix}{line}")?;
                        }
                    }
                    "package" => {
                        writeln!(f, "{prefix}{branch}{name}/")?;
                        let subpackage = &package.packages()[&SystemPathBuf::from(name)];
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

#[cfg(test)]
impl std::fmt::Debug for DisplayDiscoveredPackage<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.package.display())
    }
}

#[cfg(test)]
impl PartialEq<String> for DisplayDiscoveredPackage<'_> {
    fn eq(&self, other: &String) -> bool {
        self.to_string() == *other
    }
}

#[cfg(test)]
impl PartialEq<&str> for DisplayDiscoveredPackage<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.to_string() == *other
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use karva_project::{project::Project, testing::TestEnv};

    use super::*;

    #[test]
    fn test_update_package() {
        let env = TestEnv::with_files([("<test>/test_1.py", "")]);

        let tests_dir = env.mapped_path("<test>").unwrap();

        let project = Project::new(env.cwd(), vec![tests_dir.clone()]);

        let mut package = DiscoveredPackage::new(env.cwd());

        package.add_module(DiscoveredModule::new(
            &project,
            &tests_dir.join("test_1.py"),
            ModuleType::Test,
        ));

        assert_snapshot!(package.display().to_string(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test_1
                ├── test_cases []
                └── fixtures []
        ");
    }

    #[test]
    fn test_add_module_different_start_path() {
        let env = TestEnv::with_files([("<test>/test_1.py", ""), ("<test2>/test_1.py", "")]);

        let tests_dir = env.mapped_path("<test>").unwrap();
        let tests_dir_2 = env.mapped_path("<test2>").unwrap();

        let project = Project::new(env.cwd(), vec![tests_dir.clone(), tests_dir_2.clone()]);

        let mut package = DiscoveredPackage::new(tests_dir.clone());

        let module =
            DiscoveredModule::new(&project, &tests_dir.join("test_1.py"), ModuleType::Test);

        package.add_module(module);

        assert_snapshot!(package.display().to_string(), @r"
        └── <test>.test_1
            ├── test_cases []
            └── fixtures []
        ");
    }

    #[test]
    fn test_add_module_already_in_package() {
        let env = TestEnv::with_files([("<test>/test_1.py", "")]);

        let mapped_test_dir = env.mapped_path("<test>").unwrap();

        let project = Project::new(env.cwd(), vec![mapped_test_dir.clone()]);

        let mut package = DiscoveredPackage::new(env.cwd());

        let module = DiscoveredModule::new(
            &project,
            &mapped_test_dir.join("test_1.py"),
            ModuleType::Test,
        );

        package.add_module(module);

        let module_1 = DiscoveredModule::new(
            &project,
            &mapped_test_dir.join("test_1.py"),
            ModuleType::Test,
        );

        package.add_module(module_1);

        assert_snapshot!(package.display().to_string(), @r"
        └── <temp_dir>/<test>/
            └── <test>.test_1
                ├── test_cases []
                └── fixtures []
        ");
    }

    #[test]
    fn test_add_configuration_module() {
        let env = TestEnv::with_files([("<test>/conftest.py", "")]);

        let mapped_dir = env.mapped_path("<test>").unwrap();

        let conftest_path = mapped_dir.join("conftest.py");

        let project = Project::new(env.cwd(), vec![env.cwd()]);

        let mut package = DiscoveredPackage::new(env.cwd());

        let module = DiscoveredModule::new(&project, &conftest_path, ModuleType::Configuration);

        package.add_module(module);

        let test_package = package.get_package(mapped_dir).unwrap();

        assert_snapshot!(package.display().to_string(), @r"
        └── <temp_dir>/<test>/
            └── <test>.conftest
                ├── test_cases []
                └── fixtures []
        ");

        assert_eq!(test_package.configuration_modules().len(), 1);
        assert_eq!(
            test_package.configuration_modules()[0].path(),
            &conftest_path
        );
    }
}
