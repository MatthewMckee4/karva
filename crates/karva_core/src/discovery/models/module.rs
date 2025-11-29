use camino::Utf8PathBuf;
use ruff_source_file::{LineIndex, SourceFile, SourceFileBuilder};

use crate::{discovery::TestFunction, extensions::fixtures::Fixture, name::ModulePath};

/// A module represents a single python file.
#[derive(Debug)]
pub struct DiscoveredModule {
    path: ModulePath,
    test_functions: Vec<TestFunction>,
    fixtures: Vec<Fixture>,
    type_: ModuleType,
    source_text: String,
    line_index: LineIndex,
}

impl DiscoveredModule {
    pub(crate) fn new(path: ModulePath, module_type: ModuleType) -> Self {
        let source_text = std::fs::read_to_string(path.path()).expect("Failed to read source file");

        let line_index = LineIndex::from_source_text(&source_text);

        Self {
            path,
            test_functions: Vec::new(),
            fixtures: Vec::new(),
            type_: module_type,
            source_text,
            line_index,
        }
    }

    pub(crate) const fn module_path(&self) -> &ModulePath {
        &self.path
    }

    pub(crate) const fn path(&self) -> &Utf8PathBuf {
        self.path.path()
    }

    pub(crate) fn name(&self) -> &str {
        self.path.module_name()
    }

    pub(crate) const fn module_type(&self) -> ModuleType {
        self.type_
    }

    pub(crate) fn test_functions(&self) -> Vec<&TestFunction> {
        self.test_functions.iter().collect()
    }

    pub(crate) fn extend_test_functions(&mut self, test_functions: Vec<TestFunction>) {
        self.test_functions.extend(test_functions);
    }

    pub(crate) fn filter_test_functions(&mut self, name: &str) {
        self.test_functions.retain(|tc| tc.function_name() == name);
    }

    pub(crate) const fn fixtures(&self) -> &Vec<Fixture> {
        &self.fixtures
    }

    pub(crate) fn extend_fixtures(&mut self, fixtures: Vec<Fixture>) {
        self.fixtures.extend(fixtures);
    }

    #[cfg(test)]
    pub(crate) fn total_test_functions(&self) -> usize {
        self.test_functions.len()
    }

    pub(crate) fn source_text(&self) -> &str {
        &self.source_text
    }

    pub(crate) fn source_file(&self) -> SourceFile {
        SourceFileBuilder::new(self.path().as_str(), self.source_text()).finish()
    }

    pub(crate) const fn line_index(&self) -> &LineIndex {
        &self.line_index
    }

    pub(crate) fn update(&mut self, module: Self) {
        if self.path == module.path {
            for test_case in module.test_functions {
                if !self
                    .test_functions
                    .iter()
                    .any(|existing| existing.name() == test_case.name())
                {
                    self.test_functions.push(test_case);
                }
            }

            for fixture in module.fixtures {
                if !self
                    .fixtures
                    .iter()
                    .any(|existing| existing.name() == fixture.name())
                {
                    self.fixtures.push(fixture);
                }
            }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.test_functions.is_empty() && self.fixtures.is_empty()
    }

    #[cfg(test)]
    pub(crate) const fn display(&self) -> DisplayDiscoveredModule<'_> {
        DisplayDiscoveredModule::new(self)
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

#[cfg(test)]
pub struct DisplayDiscoveredModule<'proj> {
    module: &'proj DiscoveredModule,
}

#[cfg(test)]
impl<'proj> DisplayDiscoveredModule<'proj> {
    pub(crate) const fn new(module: &'proj DiscoveredModule) -> Self {
        Self { module }
    }
}

#[cfg(test)]
impl std::fmt::Display for DisplayDiscoveredModule<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = self.module.name();
        write!(f, "{name}\n├── test_cases [")?;
        let test_cases = self.module.test_functions();
        for (i, test) in test_cases.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "{}", test.name().function_name())?;
        }
        write!(f, "]\n└── fixtures [")?;
        let fixtures = self.module.fixtures();
        for (i, fixture) in fixtures.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "{}", fixture.name().function_name())?;
        }
        write!(f, "]")?;
        Ok(())
    }
}
