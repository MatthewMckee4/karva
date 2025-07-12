use std::{
    collections::HashSet,
    fmt::{self, Display},
};

use karva_project::{path::SystemPathBuf, project::Project, utils::module_name};

use crate::{
    discovery::TestFunction,
    fixture::{Fixture, HasFixtures, RequiresFixtures},
};

/// A module represents a single python file.
pub struct DiscoveredModule<'proj> {
    path: SystemPathBuf,
    project: &'proj Project,
    test_functions: Vec<TestFunction<'proj>>,
    fixtures: Vec<Fixture>,
    r#type: ModuleType,
}

impl<'proj> DiscoveredModule<'proj> {
    #[must_use]
    pub fn new(project: &'proj Project, path: &SystemPathBuf, module_type: ModuleType) -> Self {
        Self {
            path: path.clone(),
            project,
            test_functions: Vec::new(),
            fixtures: Vec::new(),
            r#type: module_type,
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

impl std::fmt::Debug for DiscoveredModule<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let string_module: StringModule = self.into();
        write!(f, "{string_module:?}")
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

#[derive(Debug, PartialEq, Eq)]
pub struct StringModule {
    pub test_cases: HashSet<String>,
    pub fixtures: HashSet<(String, String)>,
}

impl From<&'_ DiscoveredModule<'_>> for StringModule {
    fn from(module: &'_ DiscoveredModule<'_>) -> Self {
        Self {
            test_cases: module.test_functions().iter().map(|tc| tc.name()).collect(),
            fixtures: module
                .all_fixtures(&[])
                .into_iter()
                .map(|f| (f.name().to_string(), f.scope().to_string()))
                .collect(),
        }
    }
}
