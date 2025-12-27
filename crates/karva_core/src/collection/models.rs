use std::{collections::HashMap, sync::Arc};

use camino::Utf8PathBuf;
use ruff_python_ast::StmtFunctionDef;

use crate::name::ModulePath;

/// A collected module containing raw AST function definitions.
/// This is populated during the parallel collection phase.
#[derive(Debug, Clone)]
pub struct CollectedModule {
    /// The path of the module.
    pub(crate) path: ModulePath,
    /// The type of module.
    pub(crate) module_type: ModuleType,
    /// The source text of the file (cached to avoid re-reading)
    pub(crate) source_text: String,
    /// Test function definitions (functions starting with test prefix)
    pub(crate) test_function_defs: Vec<Arc<StmtFunctionDef>>,
    /// Fixture function definitions (functions with fixture decorators)
    pub(crate) fixture_function_defs: Vec<Arc<StmtFunctionDef>>,
}

impl CollectedModule {
    pub(crate) const fn new(
        path: ModulePath,

        module_type: ModuleType,
        source_text: String,
    ) -> Self {
        Self {
            path,

            module_type,
            source_text,
            test_function_defs: Vec::new(),
            fixture_function_defs: Vec::new(),
        }
    }

    pub(crate) fn add_test_function_def(&mut self, function_def: Arc<StmtFunctionDef>) {
        self.test_function_defs.push(function_def);
    }

    pub(crate) fn add_fixture_function_def(&mut self, function_def: Arc<StmtFunctionDef>) {
        self.fixture_function_defs.push(function_def);
    }

    pub(crate) const fn file_path(&self) -> &Utf8PathBuf {
        self.path.path()
    }

    pub(crate) const fn module_type(&self) -> ModuleType {
        self.module_type
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.test_function_defs.is_empty() && self.fixture_function_defs.is_empty()
    }
}

/// A collected package containing collected modules and subpackages.
/// This is populated during the parallel collection phase.
#[derive(Debug, Clone)]
pub struct CollectedPackage {
    pub(crate) path: Utf8PathBuf,
    pub(crate) modules: HashMap<Utf8PathBuf, CollectedModule>,
    pub(crate) packages: HashMap<Utf8PathBuf, Self>,
    pub(crate) configuration_module_path: Option<Utf8PathBuf>,
}

impl CollectedPackage {
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

    /// Add a module to this package.
    ///
    /// If the module path does not start with our path, do nothing.
    ///
    /// If the module path equals our path, use update method.
    ///
    /// Otherwise, strip the current path from the start and add the module to the appropriate sub-package.
    pub(crate) fn add_module(&mut self, module: CollectedModule) {
        if !module.file_path().starts_with(self.path()) {
            return;
        }

        if module.is_empty() {
            return;
        }

        let Some(parent_path) = module.file_path().parent() else {
            return;
        };

        if parent_path == self.path() {
            if let Some(existing_module) = self.modules.get_mut(module.file_path()) {
                existing_module.update(module);
            } else {
                if module.module_type() == ModuleType::Configuration {
                    self.configuration_module_path = Some(module.file_path().clone());
                }
                self.modules.insert(module.file_path().clone(), module);
            }
            return;
        }

        let Ok(relative_path) = module.file_path().strip_prefix(self.path()) else {
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

    pub(crate) fn add_configuration_module(&mut self, module: CollectedModule) {
        self.configuration_module_path = Some(module.file_path().clone());
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

    pub(crate) fn update(&mut self, package: Self) {
        for (_, module) in package.modules {
            self.add_module(module);
        }
        for (_, package) in package.packages {
            self.add_package(package);
        }

        if let Some(module_path) = package.configuration_module_path {
            self.configuration_module_path = Some(module_path);
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.modules.is_empty() && self.packages.is_empty()
    }

    pub(crate) fn shrink(&mut self) {
        self.modules.retain(|_, module| !module.is_empty());

        if let Some(configuration_module) = self.configuration_module_path.as_ref() {
            if !self.modules.contains_key(configuration_module) {
                self.configuration_module_path = None;
            }
        }

        self.packages.retain(|_, package| !package.is_empty());

        for package in self.packages.values_mut() {
            package.shrink();
        }
    }
}

impl CollectedModule {
    /// Update this module with another module.
    /// Merges function definitions from the other module into this one.
    pub(crate) fn update(&mut self, module: Self) {
        if self.path == module.path {
            for function_def in module.test_function_defs {
                if !self
                    .test_function_defs
                    .iter()
                    .any(|existing| existing.name == function_def.name)
                {
                    self.test_function_defs.push(function_def);
                }
            }
            for function_def in module.fixture_function_defs {
                if !self
                    .fixture_function_defs
                    .iter()
                    .any(|existing| existing.name == function_def.name)
                {
                    self.fixture_function_defs.push(function_def);
                }
            }
        }
    }
}

/// The type of module.
/// This is used to differentiation between files that contain only test functions and files that contain only configuration functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleType {
    Test,
    Configuration,
}

impl From<&Utf8PathBuf> for ModuleType {
    fn from(path: &Utf8PathBuf) -> Self {
        if path
            .file_name()
            .is_some_and(|file_name| file_name == "conftest.py")
        {
            Self::Configuration
        } else {
            Self::Test
        }
    }
}
