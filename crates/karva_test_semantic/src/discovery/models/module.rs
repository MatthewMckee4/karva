use camino::Utf8PathBuf;
use karva_python_semantic::ModulePath;
use ruff_source_file::{SourceFile, SourceFileBuilder};

use crate::discovery::DiscoveredTestFunction;
use crate::extensions::fixtures::DiscoveredFixture;

/// Represents a single Python file containing tests and/or fixtures.
///
/// Holds the discovered test functions and fixtures along with the
/// source text needed for diagnostic reporting.
#[derive(Debug)]
pub struct DiscoveredModule {
    /// The module's path information including file path and module name.
    path: ModulePath,

    /// Test functions discovered in this module.
    test_functions: Vec<DiscoveredTestFunction>,

    /// Fixture definitions discovered in this module.
    fixtures: Vec<DiscoveredFixture>,

    /// Original source code for diagnostic reporting.
    source_file: SourceFile,
}

impl DiscoveredModule {
    pub(crate) fn new_with_source(path: ModulePath, source_text: String) -> Self {
        let source_file = SourceFileBuilder::new(path.path().as_str(), source_text).finish();
        Self {
            path,
            test_functions: Vec::new(),
            fixtures: Vec::new(),
            source_file,
        }
    }

    pub(crate) fn module_path(&self) -> &ModulePath {
        &self.path
    }

    pub(crate) fn path(&self) -> &Utf8PathBuf {
        self.path.path()
    }

    pub(crate) fn name(&self) -> &str {
        self.path.module_name()
    }

    pub(crate) fn test_functions(&self) -> &[DiscoveredTestFunction] {
        &self.test_functions
    }

    pub(crate) fn add_test_function(&mut self, test_function: DiscoveredTestFunction) {
        self.test_functions.push(test_function);
    }

    pub(crate) fn fixtures(&self) -> &[DiscoveredFixture] {
        &self.fixtures
    }

    pub(crate) fn add_fixture(&mut self, fixture: DiscoveredFixture) {
        self.fixtures.push(fixture);
    }

    pub(crate) fn source_file(&self) -> SourceFile {
        self.source_file.clone()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.test_functions.is_empty() && self.fixtures.is_empty()
    }

    pub(crate) fn shrink(&mut self) {
        self.test_functions
            .sort_by_key(|function| function.stmt_function_def.range.start());
    }
}
