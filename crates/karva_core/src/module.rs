use std::{
    fmt::{self, Display},
    hash::{Hash, Hasher},
};

use karva_project::{path::SystemPathBuf, project::Project, utils::module_name};
use ruff_text_size::TextSize;

use crate::{case::TestCase, discovery::visitor::source_text, utils::from_text_size};

/// A module represents a single python file.
#[derive(Clone)]
pub struct Module<'proj> {
    path: SystemPathBuf,
    project: &'proj Project,
    test_cases: Vec<TestCase>,
}

impl<'proj> Module<'proj> {
    #[must_use]
    pub fn new(path: &SystemPathBuf, project: &'proj Project, test_cases: Vec<TestCase>) -> Self {
        Self {
            path: path.clone(),
            project,
            test_cases,
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

    pub fn test_cases(&mut self) -> &[TestCase] {
        &self.test_cases
    }

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

    pub fn update(&mut self, module: Module<'proj>) {
        if self.path == module.path {
            self.test_cases.extend(module.test_cases);
        }
    }
}

impl fmt::Debug for Module<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Module")
            .field("file", &self.path)
            .field("functions", &self.test_cases)
            .finish()
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
