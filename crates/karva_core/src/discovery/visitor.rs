use karva_project::{path::SystemPathBuf, project::Project};
use pyo3::Python;
use ruff_python_ast::{
    ModModule, PythonVersion, Stmt,
    visitor::source_order::{self, SourceOrderVisitor},
};
use ruff_python_parser::{Mode, ParseOptions, Parsed, parse_unchecked};

use crate::{
    case::TestCase,
    diagnostic::Diagnostic,
    fixture::{Fixture, FixtureExtractor, is_fixture_function},
};

pub struct FunctionDefinitionVisitor<'a> {
    discovered_functions: Vec<TestCase>,
    fixture_definitions: Vec<Fixture>,
    project: &'a Project,
    path: &'a SystemPathBuf,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> FunctionDefinitionVisitor<'a> {
    #[must_use]
    pub const fn new(project: &'a Project, path: &'a SystemPathBuf) -> Self {
        Self {
            discovered_functions: Vec::new(),
            fixture_definitions: Vec::new(),
            project,
            path,
            diagnostics: Vec::new(),
        }
    }
}

impl<'a> SourceOrderVisitor<'a> for FunctionDefinitionVisitor<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if let Stmt::FunctionDef(function_def) = stmt {
            if is_fixture_function(function_def) {
                Python::with_gil(|py| {
                    match FixtureExtractor::try_from_function(
                        &py,
                        function_def,
                        self.path,
                        self.project.cwd(),
                    ) {
                        Ok(fixture_def) => self.fixture_definitions.push(fixture_def),
                        Err(e) => {
                            self.diagnostics
                                .push(Diagnostic::invalid_fixture(&e, &self.path.to_string()));
                        }
                    }
                });
            } else if function_def
                .name
                .to_string()
                .starts_with(self.project.test_prefix())
            {
                self.discovered_functions.push(TestCase::new(
                    self.project.cwd(),
                    self.path.clone(),
                    function_def.clone(),
                ));
            }
        }

        source_order::walk_stmt(self, stmt);
    }
}

#[derive(Debug)]
pub struct DiscoveredFunctions {
    pub functions: Vec<TestCase>,
    pub fixtures: Vec<Fixture>,
}

impl DiscoveredFunctions {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.fixtures.is_empty()
    }
}

#[must_use]
pub fn discover(path: &SystemPathBuf, project: &Project) -> (DiscoveredFunctions, Vec<Diagnostic>) {
    let mut visitor = FunctionDefinitionVisitor::new(project, path);

    let parsed = parsed_module(path, *project.python_version());

    visitor.visit_body(&parsed.syntax().body);

    (
        DiscoveredFunctions {
            functions: visitor.discovered_functions,
            fixtures: visitor.fixture_definitions,
        },
        visitor.diagnostics,
    )
}

#[must_use]
pub fn parsed_module(path: &SystemPathBuf, python_version: PythonVersion) -> Parsed<ModModule> {
    let mode = Mode::Module;
    let options = ParseOptions::from(mode).with_target_version(python_version);
    let source = source_text(path);

    parse_unchecked(&source, options)
        .try_into_module()
        .expect("PySourceType always parses into a module")
}

#[must_use]
pub fn source_text(path: &SystemPathBuf) -> String {
    std::fs::read_to_string(path.as_std_path()).unwrap()
}
