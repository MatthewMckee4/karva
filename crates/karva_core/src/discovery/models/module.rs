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

    pub(crate) fn is_empty(&self) -> bool {
        self.test_functions.is_empty() && self.fixtures.is_empty()
    }

    #[cfg(test)]
    pub(crate) const fn display(&self) -> DisplayDiscoveredModule<'_> {
        DisplayDiscoveredModule::new(self)
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
        let test_functions = self.module.test_functions();
        let fixtures = self.module.fixtures();

        let indent_string = "├── ";
        let last_indent_string = "└── ";

        if !test_functions.is_empty() {
            if fixtures.is_empty() {
                write!(f, "{name}\n{last_indent_string}test_cases [")?;
            } else {
                write!(f, "{name}\n{indent_string}test_cases [")?;
            }
            for (i, test) in test_functions.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{}", test.name.function_name())?;
            }
            write!(f, "]")?;
            if !fixtures.is_empty() {
                writeln!(f)?;
            }
        }
        if !fixtures.is_empty() {
            if test_functions.is_empty() {
                write!(f, "{name}\n{last_indent_string}fixtures [")?;
            } else {
                write!(f, "{last_indent_string}fixtures [")?;
            }
            for (i, fixture) in fixtures.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{}", fixture.name().function_name())?;
            }
            write!(f, "]")?;
        }
        Ok(())
    }
}
