use std::{
    fmt::{self, Display},
    hash::{Hash, Hasher},
};

use karva_project::{path::SystemPathBuf, project::Project, utils::module_name};
use ruff_python_ast::StmtFunctionDef;
use ruff_text_size::TextSize;

use crate::{
    discovery::{function_definitions, visitor::source_text},
    utils::from_text_size,
};

#[derive(Clone)]
pub struct Module<'proj> {
    file: SystemPathBuf,
    project: &'proj Project,
    functions: Option<Vec<StmtFunctionDef>>,
}

impl<'proj> Module<'proj> {
    #[must_use]
    pub fn new(path: &SystemPathBuf, project: &'proj Project) -> Self {
        Self {
            file: path.clone(),
            project,
            functions: None,
        }
    }

    #[must_use]
    pub const fn file(&self) -> &SystemPathBuf {
        &self.file
    }

    #[must_use]
    pub fn name(&self) -> String {
        module_name(self.project.cwd(), &self.file)
    }

    pub fn functions(&mut self) -> &[StmtFunctionDef] {
        if self.functions.is_none() {
            self.functions = Some(function_definitions(&self.file, self.project));
        }
        self.functions.as_ref().unwrap()
    }

    #[must_use]
    pub fn to_column_row(&self, position: TextSize) -> (usize, usize) {
        let source_text = source_text(&self.file);
        from_text_size(position, &source_text)
    }

    #[must_use]
    pub fn source_text(&self) -> String {
        source_text(&self.file)
    }

    // Optimized method that returns both position and source text in one operation
    #[must_use]
    pub fn to_column_row_with_source(&self, position: TextSize) -> ((usize, usize), String) {
        let source_text = source_text(&self.file);
        let position = from_text_size(position, &source_text);
        (position, source_text)
    }
}

impl fmt::Debug for Module<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Module")
            .field("file", &self.file)
            .field("functions", &self.functions)
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
        self.file.hash(state);
    }
}

impl PartialEq for Module<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.file == other.file && self.name() == other.name()
    }
}

impl Eq for Module<'_> {}
