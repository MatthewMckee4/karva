use pyo3::Python;
use ruff_python_ast::{
    ModModule, PythonVersion, Stmt, StmtFunctionDef,
    visitor::source_order::{self, SourceOrderVisitor},
};
use ruff_python_parser::{Mode, ParseOptions, Parsed, parse_unchecked};

use crate::{path::SystemPathBuf, project::Project};

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

    #[must_use]
    pub fn discovered_functions(&self) -> &[StmtFunctionDef] {
        &self.discovered_functions
    }
}

impl<'a> SourceOrderVisitor<'a> for FunctionDefinitionVisitor<'a> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if let Stmt::FunctionDef(function_def) = stmt {
            if function_def
                .name
                .to_string()
                .starts_with(self.project.test_prefix())
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

    let parsed = parsed_module(path);

    visitor.visit_body(&parsed.syntax().body);

    visitor.discovered_functions().to_vec()
}

fn parsed_module(path: &SystemPathBuf) -> Parsed<ModModule> {
    let python_version = current_python_version();
    let mode = Mode::Module;
    let options = ParseOptions::from(mode).with_target_version(python_version);
    let source = source_text(path);

    parse_unchecked(&source, options)
        .try_into_module()
        .expect("PySourceType always parses into a module")
}

fn source_text(path: &SystemPathBuf) -> String {
    std::fs::read_to_string(path.as_std_path()).unwrap()
}

fn current_python_version() -> PythonVersion {
    PythonVersion::from(Python::with_gil(|py| {
        let inferred_python_version = py.version_info();
        (inferred_python_version.major, inferred_python_version.minor)
    }))
}
