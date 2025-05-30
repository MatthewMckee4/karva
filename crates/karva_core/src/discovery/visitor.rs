use std::hash::{Hash, Hasher};

use pyo3::Python;
use ruff_python_ast::{
    ModModule, PythonVersion, Stmt, StmtFunctionDef,
    visitor::source_order::{self, SourceOrderVisitor},
};
use ruff_python_parser::{Mode, ParseOptions, Parsed, parse_unchecked};

use crate::{path::SystemPathBuf, project::Project, utils::module_name};

#[derive(Clone)]
pub struct ParsedModule<'proj> {
    path: SystemPathBuf,
    name: String,
    parsed: &'proj Parsed<ModModule>,
}

impl<'proj> ParsedModule<'proj> {
    pub fn new(path: &SystemPathBuf, cwd: &SystemPathBuf) -> Self {
        let parsed = parsed_module(path);
        let name = module_name(cwd, path);
        Self {
            path: path.clone(),
            name,
            parsed: &parsed,
        }
    }

    pub fn syntax(&self) -> &ModModule {
        self.parsed.syntax()
    }

    pub fn discover_functions<'proj>(
        &'proj self,
        project: &'proj Project,
    ) -> FunctionDefinitions<'proj> {
        let mut visitor = FunctionDefinitionVisitor::new(project);
        visitor.visit_body(&self.syntax().body);

        FunctionDefinitions {
            module: self,
            discovered_functions: visitor.discovered_functions().to_vec(),
        }
    }
}

impl Hash for ParsedModule {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.name.hash(state);
    }
}

pub struct FunctionDefinitions<'proj> {
    module: &'proj ParsedModule,
    discovered_functions: Vec<&'proj StmtFunctionDef>,
}

impl<'proj> FunctionDefinitions<'proj> {
    pub fn discovered_functions(&self) -> &[&'proj StmtFunctionDef] {
        &self.discovered_functions
    }
}

#[derive(Clone)]
pub struct FunctionDefinitionVisitor<'proj> {
    discovered_functions: Vec<&'proj StmtFunctionDef>,
    project: &'proj Project,
}

impl<'proj> FunctionDefinitionVisitor<'proj> {
    #[must_use]
    pub const fn new(project: &'proj Project) -> Self {
        Self {
            discovered_functions: Vec::new(),
            project,
        }
    }

    #[must_use]
    pub fn discovered_functions(&self) -> &[&'proj StmtFunctionDef] {
        &self.discovered_functions
    }
}

impl<'proj> SourceOrderVisitor<'proj> for FunctionDefinitionVisitor<'proj> {
    fn visit_stmt(&mut self, stmt: &'proj Stmt) {
        if let Stmt::FunctionDef(function_def) = stmt {
            if function_def
                .name
                .to_string()
                .starts_with(self.project.test_prefix())
            {
                self.discovered_functions.push(&function_def);
            }
        }

        source_order::walk_stmt(self, stmt);
    }
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
