use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

use karva_project::{path::SystemPathBuf, project::Project};

use crate::module::Module;

/// A package represents a single python directory.
#[derive(Debug, Clone)]
pub struct Package<'proj> {
    path: SystemPathBuf,
    project: &'proj Project,
    modules: HashMap<SystemPathBuf, Module<'proj>>,
}

impl<'proj> Package<'proj> {
    #[must_use]
    pub fn new(path: SystemPathBuf, project: &'proj Project) -> Self {
        Self {
            path,
            project,
            modules: HashMap::new(),
        }
    }

    #[must_use]
    pub const fn path(&self) -> &SystemPathBuf {
        &self.path
    }

    pub fn modules(&self) -> &HashMap<SystemPathBuf, Module<'proj>> {
        &self.modules
    }

    pub fn add_module(&mut self, module: Module<'proj>) {
        self.modules.insert(module.path().clone(), module);
    }

    pub fn total_test_cases(&self) -> usize {
        self.modules.values().map(Module::total_test_cases).sum()
    }

    pub fn update(&mut self, module: Package<'proj>) {
        self.modules.extend(module.modules);
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
