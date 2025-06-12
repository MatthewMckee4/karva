use std::collections::HashMap;

use karva_project::path::SystemPathBuf;

use crate::{
    fixture::{Fixture, HasFixtures},
    module::{Module, ModuleType},
    package::Package,
};

#[derive(Debug)]
pub struct Session<'proj> {
    modules: HashMap<SystemPathBuf, Module<'proj>>,
    packages: HashMap<SystemPathBuf, Package<'proj>>,
}

impl<'proj> Session<'proj> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
            packages: HashMap::new(),
        }
    }

    #[must_use]
    pub const fn packages(&self) -> &HashMap<SystemPathBuf, Package<'proj>> {
        &self.packages
    }

    #[must_use]
    pub const fn modules(&self) -> &HashMap<SystemPathBuf, Module<'proj>> {
        &self.modules
    }

    pub fn total_test_cases(&self) -> usize {
        let package_test_cases = self
            .packages
            .values()
            .map(Package::total_test_cases)
            .sum::<usize>();
        let module_test_cases = self
            .modules
            .values()
            .map(Module::total_test_cases)
            .sum::<usize>();
        package_test_cases + module_test_cases
    }

    pub fn add_package(&mut self, package: Package<'proj>) {
        if let Some(existing_package) = self.packages.get_mut(package.path()) {
            existing_package.update(package);
        } else {
            self.packages.insert(package.path().clone(), package);
        }
    }

    pub fn add_module(&mut self, module: Module<'proj>) {
        if let Some(existing_module) = self.modules.get_mut(module.path()) {
            existing_module.update(module);
        } else {
            self.modules.insert(module.path().clone(), module);
        }
    }

    #[must_use]
    pub fn configuration_modules(&self) -> Vec<&Module<'_>> {
        self.modules
            .values()
            .filter(|m| m.module_type == ModuleType::Configuration)
            .collect::<Vec<_>>()
    }
}

impl Default for Session<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl HasFixtures for Session<'_> {
    fn all_fixtures(&self) -> Vec<&Fixture> {
        self.configuration_modules()
            .iter()
            .flat_map(|m| &m.fixtures)
            .collect::<Vec<_>>()
    }
}
