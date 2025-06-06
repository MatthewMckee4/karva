use karva_project::{path::SystemPathBuf, project::Project};
use ruff_python_ast::{
    ModModule, PythonVersion, Stmt, StmtFunctionDef,
    visitor::source_order::{self, SourceOrderVisitor},
};
use ruff_python_parser::{Mode, ParseOptions, Parsed, parse_unchecked};

use crate::fixture::is_fixture_function;

#[derive(Clone)]
pub struct FunctionDefinitionVisitor<'a> {
    discovered_functions: Vec<StmtFunctionDef>,
    project: &'a Project,
}

impl<'a> FunctionDefinitionVisitor<'a> {
    #[must_use]
    pub const fn new(project: &'a Project) -> Self {
        Self {
            discovered_functions: Vec::new(),
            project,
        }
    }
}

impl<'a> SourceOrderVisitor<'a> for FunctionDefinitionVisitor<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if let Stmt::FunctionDef(function_def) = stmt {
            if function_def
                .name
                .to_string()
                .starts_with(self.project.test_prefix())
                && !is_fixture_function(function_def)
            {
                self.discovered_functions.push(function_def.clone());
            }
        }

        source_order::walk_stmt(self, stmt);
    }
}

#[must_use]
pub fn function_definitions(path: &SystemPathBuf, project: &Project) -> Vec<StmtFunctionDef> {
    let mut visitor = FunctionDefinitionVisitor::new(project);

    let parsed = parsed_module(path, *project.python_version());

    visitor.visit_body(&parsed.syntax().body);

    visitor.discovered_functions
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
