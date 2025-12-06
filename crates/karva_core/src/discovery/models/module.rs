use camino::Utf8PathBuf;
use ruff_source_file::{SourceFile, SourceFileBuilder};

use crate::{discovery::TestFunction, extensions::fixtures::Fixture, name::ModulePath};

/// A module represents a single python file.
#[derive(Debug)]
pub struct DiscoveredModule {
    path: ModulePath,
    test_functions: Vec<TestFunction>,
    fixtures: Vec<Fixture>,
    source_text: String,
}

impl DiscoveredModule {
    pub(crate) const fn new_with_source(path: ModulePath, source_text: String) -> Self {
        Self {
            path,
            test_functions: Vec::new(),
            fixtures: Vec::new(),
            source_text,
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

    pub(crate) fn test_functions(&self) -> Vec<&TestFunction> {
        self.test_functions.iter().collect()
    }

    pub(crate) fn add_test_function(&mut self, test_function: TestFunction) {
        self.test_functions.push(test_function);
    }

    pub(crate) const fn fixtures(&self) -> &Vec<Fixture> {
        &self.fixtures
    }

    pub(crate) fn add_fixture(&mut self, fixture: Fixture) {
        self.fixtures.push(fixture);
    }

    pub(crate) fn source_text(&self) -> &str {
        &self.source_text
    }

    pub(crate) fn source_file(&self) -> SourceFile {
        SourceFileBuilder::new(self.path().as_str(), self.source_text()).finish()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.test_functions.is_empty() && self.fixtures.is_empty()
    }
}
