use camino::Utf8PathBuf;
use karva_python_semantic::ModulePath;
use ruff_source_file::{SourceFile, SourceFileBuilder};

use crate::discovery::TestFunction;
use crate::extensions::fixtures::Fixture;

/// Represents a single Python file containing tests and/or fixtures.
///
/// Holds the discovered test functions and fixtures along with the
/// source text needed for diagnostic reporting.
#[derive(Debug)]
pub struct DiscoveredModule {
    /// The module's path information including file path and module name.
    path: ModulePath,

    /// Test functions discovered in this module.
    test_functions: Vec<TestFunction>,

    /// Fixture definitions discovered in this module.
    fixtures: Vec<Fixture>,

    /// Original source code text for diagnostic reporting.
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

    pub(crate) fn test_functions(&self) -> &Vec<TestFunction> {
        &self.test_functions
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

    pub(crate) fn shrink(&mut self) {
        self.test_functions
            .sort_by_key(|function| function.stmt_function_def.range.start());
    }
}
