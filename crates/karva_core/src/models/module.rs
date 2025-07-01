use std::{
    collections::HashSet,
    fmt::{self, Display},
    hash::{Hash, Hasher},
};

use karva_project::{path::SystemPathBuf, project::Project, utils::module_name};
use ruff_python_ast::{ModModule, PythonVersion};
use ruff_python_parser::{Mode, ParseOptions, Parsed, parse_unchecked};
use ruff_text_size::TextSize;

use crate::{
    discovery::visitor::source_text,
    fixture::{Fixture, HasFixtures, RequiresFixtures},
    models::TestFunction,
    utils::from_text_size,
};

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
        if path.file_name() == Some("conftest.py") {
            Self::Configuration
        } else {
            Self::Test
        }
    }
}

/// A module represents a single python file.
pub struct Module<'proj> {
    pub path: SystemPathBuf,
    pub project: &'proj Project,
    test_cases: Vec<TestFunction>,
    fixtures: Vec<Fixture>,
    pub module_type: ModuleType,
}

impl<'proj> Module<'proj> {
    #[must_use]
    pub fn new(project: &'proj Project, path: &SystemPathBuf, module_type: ModuleType) -> Self {
        Self {
            path: path.clone(),
            project,
            test_cases: Vec::new(),
            fixtures: Vec::new(),
            module_type,
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
    pub fn test_cases(&self) -> Vec<&TestFunction> {
        self.test_cases.iter().collect()
    }

    pub fn set_test_cases(&mut self, test_cases: Vec<TestFunction>) {
        self.test_cases = test_cases;
    }

    #[must_use]
    pub fn get_test_case(&self, name: &str) -> Option<&TestFunction> {
        self.test_cases.iter().find(|tc| tc.name() == name)
    }

    #[must_use]
    pub fn fixtures(&self) -> Vec<&Fixture> {
        self.fixtures.iter().collect()
    }

    pub fn set_fixtures(&mut self, fixtures: Vec<Fixture>) {
        self.fixtures = fixtures;
    }

    #[must_use]
    pub fn total_test_cases(&self) -> usize {
        self.test_cases.len()
    }

    #[must_use]
    pub fn to_column_row(&self, position: TextSize) -> (usize, usize) {
        let source_text = source_text(&self.path);
        from_text_size(position, &source_text)
    }

    #[must_use]
    pub fn source_text(&self) -> String {
        source_text(&self.path)
    }

    // Optimized method that returns both position and source text in one operation
    #[must_use]
    pub fn to_column_row_with_source(&self, position: TextSize) -> ((usize, usize), String) {
        let source_text = source_text(&self.path);
        let position = from_text_size(position, &source_text);
        (position, source_text)
    }

    pub fn update(&mut self, module: Self) {
        if self.path == module.path {
            for test_case in module.test_cases {
                if !self
                    .test_cases
                    .iter()
                    .any(|existing| existing.name() == test_case.name())
                {
                    self.test_cases.push(test_case);
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
        for tc in &self.test_cases {
            deps.push(tc as &dyn RequiresFixtures);
        }
        for f in &self.fixtures {
            deps.push(f as &dyn RequiresFixtures);
        }
        deps
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.test_cases.is_empty() && self.fixtures.is_empty()
    }

    #[must_use]
    pub fn parsed_module(&self, python_version: PythonVersion) -> Parsed<ModModule> {
        let mode = Mode::Module;
        let options = ParseOptions::from(mode).with_target_version(python_version);
        let source = source_text(self.path());

        parse_unchecked(&source, options)
            .try_into_module()
            .expect("PySourceType always parses into a module")
    }
}

impl<'proj> HasFixtures<'proj> for Module<'proj> {
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
            .filter(|f| test_cases.iter().any(|tc| tc.uses_fixture(f.name())))
            .collect();

        all_fixtures
    }
}

impl Display for Module<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl Hash for Module<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
    }
}

impl PartialEq for Module<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.name() == other.name()
    }
}

impl Eq for Module<'_> {}

impl std::fmt::Debug for Module<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let string_module: StringModule = self.into();
        write!(f, "{string_module:?}")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct StringModule {
    pub test_cases: HashSet<String>,
    pub fixtures: HashSet<(String, String)>,
}

impl From<&'_ Module<'_>> for StringModule {
    fn from(module: &'_ Module<'_>) -> Self {
        Self {
            test_cases: module.test_cases().iter().map(|tc| tc.name()).collect(),
            fixtures: module
                .all_fixtures(&[])
                .into_iter()
                .map(|f| (f.name().to_string(), f.scope().to_string()))
                .collect(),
        }
    }
}
