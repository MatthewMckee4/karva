use camino::Utf8PathBuf;
use pyo3::Python;
use ruff_source_file::LineIndex;

use crate::{
    discovery::TestFunction,
    extensions::fixtures::{Fixture, HasFixtures, UsesFixtures},
    name::ModulePath,
};

/// A module represents a single python file.
#[derive(Debug)]
pub(crate) struct DiscoveredModule {
    path: ModulePath,
    test_functions: Vec<TestFunction>,
    fixtures: Vec<Fixture>,
    type_: ModuleType,
    source_text: String,
}

impl DiscoveredModule {
    pub(crate) fn new(path: ModulePath, module_type: ModuleType) -> Self {
        let source_text =
            std::fs::read_to_string(path.module_path()).expect("Failed to read source file");

        Self {
            path,
            test_functions: Vec::new(),
            fixtures: Vec::new(),
            type_: module_type,
            source_text,
        }
    }

    pub(crate) const fn path(&self) -> &Utf8PathBuf {
        self.path.module_path()
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

    pub(crate) fn with_test_functions(self, test_functions: Vec<TestFunction>) -> Self {
        Self {
            test_functions,
            ..self
        }
    }

    pub(crate) fn filter_test_functions(&mut self, name: &str) {
        self.test_functions.retain(|tc| tc.function_name() == name);
    }

    #[cfg(test)]
    pub(crate) fn get_test_function(&self, name: &str) -> Option<&TestFunction> {
        self.test_functions
            .iter()
            .find(|tc| tc.function_name() == name)
    }

    #[cfg(test)]
    pub(crate) fn fixtures(&self) -> Vec<&Fixture> {
        self.fixtures.iter().collect()
    }

    pub(crate) fn with_fixtures(self, fixtures: Vec<Fixture>) -> Self {
        Self { fixtures, ..self }
    }

    pub(crate) fn total_test_functions(&self) -> usize {
        self.test_functions.len()
    }

    pub(crate) const fn source_text(&self) -> &String {
        &self.source_text
    }

    pub(crate) fn line_index(&self) -> LineIndex {
        let source_text = self.source_text();
        LineIndex::from_source_text(source_text)
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

    pub(crate) fn all_uses_fixtures(&self) -> Vec<&dyn UsesFixtures> {
        let mut deps = Vec::new();
        for tc in &self.test_functions {
            deps.push(tc as &dyn UsesFixtures);
        }
        for f in &self.fixtures {
            deps.push(f as &dyn UsesFixtures);
        }
        deps
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.test_functions.is_empty() && self.fixtures.is_empty()
    }

    #[cfg(test)]
    pub(crate) const fn display(&self) -> DisplayDiscoveredModule<'_> {
        DisplayDiscoveredModule::new(self)
    }
}

impl<'proj> HasFixtures<'proj> for DiscoveredModule {
    fn all_fixtures<'a: 'proj>(
        &'a self,
        py: Python<'_>,
        test_cases: &[&dyn UsesFixtures],
    ) -> Vec<&'proj Fixture> {
        if test_cases.is_empty() {
            return self.fixtures.iter().collect();
        }

        let all_fixtures: Vec<&'proj Fixture> = self
            .fixtures
            .iter()
            .filter(|f| {
                if f.auto_use() {
                    true
                } else {
                    test_cases
                        .iter()
                        .any(|tc| tc.uses_fixture(py, f.name().function_name()))
                }
            })
            .collect();

        all_fixtures
    }
}

/// The type of module.
/// This is used to differentiation between files that contain only test functions and files that contain only configuration functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModuleType {
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
pub(crate) struct DisplayDiscoveredModule<'proj> {
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

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use karva_project::project::Project;
    use karva_test::TestContext;
    use pyo3::prelude::*;

    use crate::discovery::StandardDiscoverer;

    #[test]
    fn test_display_discovered_module() {
        let env = TestContext::with_files([("<test>/test.py", "def test_display(): pass")]);

        let mapped_dir = env.mapped_path("<test>").unwrap();

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::attach(|py| StandardDiscoverer::new(&project).discover(py));

        let test_module = session.get_module(&mapped_dir.join("test.py")).unwrap();

        assert_snapshot!(
            test_module.display().to_string(),
            @r"
            <test>.test
            ├── test_cases [test_display]
            └── fixtures []
            "
        );
    }
}
