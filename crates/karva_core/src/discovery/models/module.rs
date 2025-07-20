use std::fmt::{self, Display};

use karva_project::{path::SystemPathBuf, project::Project, utils::module_name};
use ruff_source_file::LineIndex;

use crate::{
    discovery::TestFunction,
    extensions::fixtures::{Fixture, HasFixtures, RequiresFixtures},
};

/// A module represents a single python file.
#[derive(Debug)]
pub struct DiscoveredModule<'proj> {
    path: SystemPathBuf,
    test_functions: Vec<TestFunction<'proj>>,
    fixtures: Vec<Fixture>,
    r#type: ModuleType,
    name: String,
}

impl<'proj> DiscoveredModule<'proj> {
    #[must_use]
    pub fn new(project: &'proj Project, path: &SystemPathBuf, module_type: ModuleType) -> Self {
        Self {
            path: path.clone(),
            test_functions: Vec::new(),
            fixtures: Vec::new(),
            r#type: module_type,
            name: module_name(project.cwd(), path).expect("Module has no name"),
        }
    }

    #[must_use]
    pub const fn path(&self) -> &SystemPathBuf {
        &self.path
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn module_type(&self) -> ModuleType {
        self.r#type
    }

    #[must_use]
    pub fn test_functions(&self) -> Vec<&TestFunction<'proj>> {
        self.test_functions.iter().collect()
    }

    pub fn set_test_functions(&mut self, test_cases: Vec<TestFunction<'proj>>) {
        self.test_functions = test_cases;
    }

    #[must_use]
    pub fn get_test_function(&self, name: &str) -> Option<&TestFunction<'proj>> {
        self.test_functions.iter().find(|tc| tc.name() == name)
    }

    #[must_use]
    pub fn fixtures(&self) -> Vec<&Fixture> {
        self.fixtures.iter().collect()
    }

    pub fn set_fixtures(&mut self, fixtures: Vec<Fixture>) {
        self.fixtures = fixtures;
    }

    #[must_use]
    pub fn total_test_functions(&self) -> usize {
        self.test_functions.len()
    }

    #[must_use]
    pub fn source_text(&self) -> String {
        std::fs::read_to_string(self.path()).expect("Failed to read source file")
    }

    #[must_use]
    pub fn line_index(&self) -> LineIndex {
        let source_text = self.source_text();
        LineIndex::from_source_text(&source_text)
    }

    pub fn update(&mut self, module: Self) {
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

    #[must_use]
    pub fn dependencies(&self) -> Vec<&dyn RequiresFixtures> {
        let mut deps = Vec::new();
        for tc in &self.test_functions {
            deps.push(tc as &dyn RequiresFixtures);
        }
        for f in &self.fixtures {
            deps.push(f as &dyn RequiresFixtures);
        }
        deps
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.test_functions.is_empty() && self.fixtures.is_empty()
    }

    #[must_use]
    pub const fn display(&self) -> DisplayDiscoveredModule<'_> {
        DisplayDiscoveredModule::new(self)
    }
}

impl<'proj> HasFixtures<'proj> for DiscoveredModule<'proj> {
    fn all_fixtures<'a: 'proj>(
        &'a self,
        test_cases: &[&dyn RequiresFixtures],
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
                    test_cases.iter().any(|tc| tc.uses_fixture(f.name()))
                }
            })
            .collect();

        all_fixtures
    }
}

impl Display for DiscoveredModule<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// The type of module.
/// This is used to differentiation between files that contain only test functions and files that contain only configuration functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleType {
    Test,
    Configuration,
}

impl ModuleType {
    #[must_use]
    pub fn from_path(path: &SystemPathBuf) -> Self {
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

pub struct DisplayDiscoveredModule<'proj> {
    module: &'proj DiscoveredModule<'proj>,
}

impl<'proj> DisplayDiscoveredModule<'proj> {
    #[must_use]
    pub const fn new(module: &'proj DiscoveredModule<'proj>) -> Self {
        Self { module }
    }
}

impl std::fmt::Display for DisplayDiscoveredModule<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = self.module.name();
        write!(f, "{name}\n├── test_cases [")?;
        let test_cases = self.module.test_functions();
        for (i, test) in test_cases.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "{}", test.name())?;
        }
        write!(f, "]\n└── fixtures [")?;
        let fixtures = self.module.fixtures();
        for (i, fixture) in fixtures.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "{}", fixture.name())?;
        }
        write!(f, "]")?;
        Ok(())
    }
}

impl std::fmt::Debug for DisplayDiscoveredModule<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.module.display())
    }
}

impl PartialEq<String> for DisplayDiscoveredModule<'_> {
    fn eq(&self, other: &String) -> bool {
        self.to_string() == *other
    }
}

impl PartialEq<&str> for DisplayDiscoveredModule<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.to_string() == *other
    }
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;
    use karva_project::{project::Project, testing::TestEnv};
    use pyo3::prelude::*;

    use crate::discovery::StandardDiscoverer;

    #[test]
    fn test_display_discovered_module() {
        let env = TestEnv::with_files([("<test>/test.py", "def test_display(): pass")]);

        let mapped_dir = env.mapped_path("<test>").unwrap();

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| StandardDiscoverer::new(&project).discover(py));

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
