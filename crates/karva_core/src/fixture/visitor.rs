use karva_project::{path::SystemPathBuf, project::Project};
use pyo3::Python;
use ruff_python_ast::{
    Stmt,
    visitor::source_order::{self, SourceOrderVisitor},
};

use crate::{discovery::visitor::parsed_module, fixture::Fixture};

pub struct FixtureDefinitionVisitor<'a> {
    py: &'a Python<'a>,
    fixture_definitions: Vec<Fixture>,
    project: &'a Project,
    path: &'a SystemPathBuf,
}

impl<'a> FixtureDefinitionVisitor<'a> {
    #[must_use]
    const fn new(py: &'a Python<'a>, project: &'a Project, path: &'a SystemPathBuf) -> Self {
        Self {
            py,
            fixture_definitions: Vec::new(),
            project,
            path,
        }
    }
}

impl<'a> SourceOrderVisitor<'a> for FixtureDefinitionVisitor<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if let Stmt::FunctionDef(function_def) = stmt {
            match Fixture::from(self.py, function_def, self.path, self.project.cwd()) {
                Ok(fixture_def) => self.fixture_definitions.push(fixture_def),
                Err(e) => tracing::debug!("Skipping non-fixture function: {}", e),
            }
        }

        source_order::walk_stmt(self, stmt);
    }
}

#[must_use]
pub fn fixture_definitions(
    py: &Python<'_>,
    path: &SystemPathBuf,
    project: &Project,
) -> Vec<Fixture> {
    let mut visitor = FixtureDefinitionVisitor::new(py, project, path);

    let parsed = parsed_module(path, *project.python_version());

    visitor.visit_body(&parsed.syntax().body);

    visitor.fixture_definitions
}
