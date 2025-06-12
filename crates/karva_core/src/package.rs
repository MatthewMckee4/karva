use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

use karva_project::path::SystemPathBuf;

use crate::{
    case::TestCase,
    fixture::Fixture,
    module::{Module, ModuleType},
};

pub trait HasFixtures {
    fn fixtures(&self) -> Vec<Fixture>;
}

/// A package represents a single python directory.
#[derive(Debug)]
pub struct Package<'proj> {
    path: SystemPathBuf,
    modules: HashMap<SystemPathBuf, Module<'proj>>,
    sub_packages: HashMap<SystemPathBuf, Package<'proj>>,
}

impl<'proj> Package<'proj> {
    #[must_use]
    pub fn new(path: SystemPathBuf) -> Self {
        Self {
            path,
            modules: HashMap::new(),
            sub_packages: HashMap::new(),
        }
    }

    #[must_use]
    pub const fn path(&self) -> &SystemPathBuf {
        &self.path
    }

    #[must_use]
    pub const fn modules(&self) -> &HashMap<SystemPathBuf, Module<'proj>> {
        &self.modules
    }

    #[must_use]
    pub const fn sub_packages(&self) -> &HashMap<SystemPathBuf, Self> {
        &self.sub_packages
    }

    pub fn add_module(&mut self, module: Module<'proj>) {
        if !module.path().starts_with(self.path()) {
            return;
        }

        if let Some(existing_module) = self.modules.get_mut(module.path()) {
            existing_module.update(module);
        } else {
            self.modules.insert(module.path().clone(), module);
        }
    }

    pub fn add_sub_package(&mut self, package: Self) {
        if !package.path().starts_with(self.path()) {
            return;
        }
        if let Some(existing_package) = self.sub_packages.get_mut(package.path()) {
            existing_package.update(package);
        } else {
            self.sub_packages.insert(package.path().clone(), package);
        }
    }

    pub fn total_test_cases(&self) -> usize {
        self.modules.values().map(Module::total_test_cases).sum()
    }

    pub fn update(&mut self, package: Self) {
        self.modules.extend(package.modules);
        self.sub_packages.extend(package.sub_packages);
    }

    #[must_use]
    pub fn test_cases(&self) -> Vec<&TestCase> {
        let mut cases = self
            .modules
            .values()
            .flat_map(|m| &m.test_cases)
            .collect::<Vec<_>>();
        for sub_package in self.sub_packages.values() {
            cases.extend(sub_package.test_cases());
        }
        cases
    }

    #[must_use]
    pub fn configuration_modules(&self) -> Vec<&Module<'_>> {
        self.modules
            .values()
            .filter(|m| m.module_type == ModuleType::Configuration)
            .collect::<Vec<_>>()
    }

    #[must_use]
    pub fn fixtures(&self) -> Vec<&Fixture> {
        self.configuration_modules()
            .iter()
            .flat_map(|m| &m.fixtures)
            .collect::<Vec<_>>()
    }
}

impl Hash for Package<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
    }
}

impl PartialEq for Package<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for Package<'_> {}
