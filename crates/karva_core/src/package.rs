use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
};

use karva_project::{path::SystemPathBuf, project::Project, utils::module_name};

use crate::{
    case::TestCase,
    fixture::{Fixture, HasFixtures, RequiresFixtures},
    module::{Module, ModuleType, StringModule},
    utils::Upcast,
};

/// A package represents a single python directory.
pub struct Package<'proj> {
    path: SystemPathBuf,
    project: &'proj Project,
    modules: HashMap<SystemPathBuf, Module<'proj>>,
    packages: HashMap<SystemPathBuf, Package<'proj>>,
    configuration_modules: HashSet<SystemPathBuf>,
}

impl<'proj> Package<'proj> {
    #[must_use]
    pub fn new(path: SystemPathBuf, project: &'proj Project) -> Self {
        Self {
            path,
            project,
            modules: HashMap::new(),
            packages: HashMap::new(),
            configuration_modules: HashSet::new(),
        }
    }

    #[must_use]
    pub const fn path(&self) -> &SystemPathBuf {
        &self.path
    }

    #[must_use]
    pub fn name(&self) -> String {
        module_name(self.project.cwd(), &self.path)
    }

    #[must_use]
    pub const fn project(&self) -> &Project {
        self.project
    }

    #[must_use]
    pub const fn modules(&self) -> &HashMap<SystemPathBuf, Module<'proj>> {
        &self.modules
    }

    #[must_use]
    pub const fn packages(&self) -> &HashMap<SystemPathBuf, Self> {
        &self.packages
    }

    #[must_use]
    pub fn get_module(&self, path: &SystemPathBuf) -> Option<&Module<'proj>> {
        self.modules.get(path)
    }

    #[must_use]
    pub fn get_package(&self, path: &SystemPathBuf) -> Option<&Self> {
        self.packages.get(path)
    }

    pub fn add_module(&mut self, module: Module<'proj>) {
        if !module.path().starts_with(self.path()) {
            return;
        }

        if let Some(existing_module) = self.modules.get_mut(module.path()) {
            existing_module.update(module);
        } else {
            if module.module_type == ModuleType::Configuration {
                self.configuration_modules.insert(module.path().clone());
            }
            self.modules.insert(module.path().clone(), module);
        }
    }

    pub fn add_configuration_module(&mut self, module: Module<'proj>) {
        self.configuration_modules.insert(module.path().clone());
        self.add_module(module);
    }

    pub fn add_package(&mut self, package: Self) {
        if !package.path().starts_with(self.path()) {
            return;
        }

        let relative_path = package.path().strip_prefix(self.path()).unwrap();
        let components: Vec<_> = relative_path.components().collect();

        if components.len() <= 1 {
            if let Some(existing_package) = self.packages.get_mut(package.path()) {
                existing_package.update(package);
            } else {
                self.packages.insert(package.path().clone(), package);
            }
        } else {
            let first_component = components[0];
            let intermediate_path = self.path().join(first_component.as_str());

            let intermediate_package = self
                .packages
                .entry(intermediate_path.clone())
                .or_insert_with(|| Package::new(intermediate_path, self.project));

            intermediate_package.add_package(package);
        }
    }

    #[must_use]
    pub fn total_test_cases(&self) -> usize {
        let mut total = 0;
        for module in self.modules.values() {
            total += module.total_test_cases();
        }
        for package in self.packages.values() {
            total += package.total_test_cases();
        }
        total
    }

    #[must_use]
    pub fn total_test_modules(&self) -> usize {
        let mut total = 0;
        for module in self.modules.values() {
            if module.module_type == ModuleType::Test {
                total += 1;
            }
        }
        for package in self.packages.values() {
            total += package.total_test_modules();
        }
        total
    }

    pub fn update(&mut self, package: Self) {
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
    pub fn test_cases(&self) -> Vec<&TestCase> {
        let mut cases = self.direct_test_cases();

        for sub_package in self.packages.values() {
            cases.extend(sub_package.test_cases());
        }

        cases
    }

    #[must_use]
    pub fn direct_test_cases(&self) -> Vec<&TestCase> {
        let mut cases = Vec::new();

        for module in self.modules.values() {
            cases.extend(module.test_cases());
        }

        cases
    }

    // TODO: Rename this
    // This function returns all functions that
    #[must_use]
    pub fn dependencies(&self) -> Vec<&dyn RequiresFixtures> {
        let mut dependencies: Vec<&dyn RequiresFixtures> = Vec::new();
        let direct_test_cases: Vec<&dyn RequiresFixtures> = self.direct_test_cases().upcast();

        for configuration_module in self.configuration_modules() {
            dependencies.extend(configuration_module.dependencies());
        }
        dependencies.extend(direct_test_cases);

        dependencies
    }

    #[must_use]
    pub fn configuration_modules(&self) -> Vec<&Module<'_>> {
        self.configuration_modules
            .iter()
            .filter_map(|path| self.modules.get(path))
            .collect()
    }

    pub fn shrink(&mut self) {
        self.modules.retain(|path, module| {
            if module.is_empty() {
                self.configuration_modules.remove(path);
                false
            } else {
                true
            }
        });
        self.packages.retain(|_, package| !package.is_empty());
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty() && self.packages.is_empty()
    }

    #[must_use]
    pub fn display(&self) -> StringPackage {
        let mut modules = HashMap::new();
        let mut packages = HashMap::new();

        for module in self.modules().values() {
            modules.insert(module_name(self.path(), module.path()), module.into());
        }

        for subpackage in self.packages().values() {
            packages.insert(
                module_name(self.path(), subpackage.path()),
                subpackage.display(),
            );
        }

        StringPackage { modules, packages }
    }
}

impl<'proj> HasFixtures<'proj> for Package<'proj> {
    fn all_fixtures<'a: 'proj>(
        &'a self,
        test_cases: &[&dyn RequiresFixtures],
    ) -> Vec<&'proj Fixture> {
        let mut fixtures = Vec::new();

        for module in self.configuration_modules() {
            let module_fixtures = module.all_fixtures(test_cases);

            fixtures.extend(module_fixtures);
        }

        fixtures
    }
}

impl<'proj> HasFixtures<'proj> for &'proj Package<'proj> {
    fn all_fixtures<'a: 'proj>(
        &'a self,
        test_cases: &[&dyn RequiresFixtures],
    ) -> Vec<&'proj Fixture> {
        (*self).all_fixtures(test_cases)
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

impl<'a> Upcast<Vec<&'a dyn HasFixtures<'a>>> for Vec<&'a Package<'a>> {
    fn upcast(self) -> Vec<&'a dyn HasFixtures<'a>> {
        self.into_iter()
            .map(|p| p as &dyn HasFixtures<'a>)
            .collect()
    }
}

#[derive(Debug)]
pub struct StringPackage {
    pub modules: HashMap<String, StringModule>,
    pub packages: HashMap<String, StringPackage>,
}

impl PartialEq for StringPackage {
    fn eq(&self, other: &Self) -> bool {
        self.modules == other.modules && self.packages == other.packages
    }
}

impl Eq for StringPackage {}

impl std::fmt::Debug for Package<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let string_package: StringPackage = self.display();
        write!(f, "{string_package:?}")
    }
}
