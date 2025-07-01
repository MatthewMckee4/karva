use karva_project::{path::SystemPathBuf, project::Project, utils::module_name};
use pyo3::{prelude::*, types::PyModule};
use ruff_python_ast::{
    Expr, Stmt, StmtFunctionDef,
    visitor::source_order::{self, SourceOrderVisitor},
};

use crate::{
    diagnostic::{Diagnostic, DiagnosticScope, ErrorType, Severity},
    fixture::{Fixture, FixtureExtractor, is_fixture_function},
    models::{Module, TestFunction},
};

pub struct FunctionDefinitionVisitor<'a, 'b> {
    discovered_functions: Vec<TestFunction>,
    fixture_definitions: Vec<Fixture>,
    project: &'a Project,
    module: &'a Module<'a>,
    diagnostics: Vec<Diagnostic>,
    py_module: Bound<'b, PyModule>,
    inside_function: bool,
}

impl<'a, 'b> FunctionDefinitionVisitor<'a, 'b> {
    pub fn new(py: Python<'b>, project: &'a Project, module: &'a Module<'a>) -> PyResult<Self> {
        let module_name = module_name(project.cwd(), module.path());

        let py_module = py.import(module_name)?;

        Ok(Self {
            discovered_functions: Vec::new(),
            fixture_definitions: Vec::new(),
            project,
            module,
            diagnostics: Vec::new(),
            py_module,
            inside_function: false,
        })
    }
}

impl<'a> SourceOrderVisitor<'a> for FunctionDefinitionVisitor<'a, '_> {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        if let Stmt::FunctionDef(function_def) = stmt {
            // Only consider top-level functions (not nested)
            if self.inside_function {
                return;
            }
            self.inside_function = true;
            if is_fixture_function(function_def) {
                match FixtureExtractor::try_from_function(function_def, &self.py_module) {
                    Ok(fixture_def) => self.fixture_definitions.push(fixture_def),
                    Err(e) => {
                        self.diagnostics.push(Diagnostic::invalid_fixture(
                            e,
                            Some(self.module.path().to_string()),
                        ));
                    }
                }
            } else if function_def
                .name
                .to_string()
                .starts_with(self.project.options().test_prefix())
            {
                self.discovered_functions.push(TestFunction::new(
                    self.module.path().clone(),
                    function_def.clone(),
                ));
            }
            source_order::walk_stmt(self, stmt);

            self.inside_function = false;
            return;
        }
        // For all other statements, walk as normal
        source_order::walk_stmt(self, stmt);
    }
}

#[derive(Debug)]
pub struct DiscoveredFunctions {
    pub functions: Vec<TestFunction>,
    pub fixtures: Vec<Fixture>,
}

impl DiscoveredFunctions {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.fixtures.is_empty()
    }
}

#[must_use]
pub fn discover(
    py: Python<'_>,
    module: &Module,
    project: &Project,
) -> (DiscoveredFunctions, Vec<Diagnostic>) {
    let mut visitor = match FunctionDefinitionVisitor::new(py, project, module) {
        Ok(visitor) => visitor,
        Err(e) => {
            return (
                DiscoveredFunctions {
                    functions: Vec::new(),
                    fixtures: Vec::new(),
                },
                vec![Diagnostic::from_py_err(
                    py,
                    &e,
                    DiagnosticScope::Discovery,
                    Some(module.path().to_string()),
                    Severity::Error(ErrorType::Unknown),
                )],
            );
        }
    };

    let parsed = module.parsed_module(project.metadata().python_version());
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
pub fn source_text(path: &SystemPathBuf) -> String {
    std::fs::read_to_string(path.as_std_path()).unwrap()
}

pub fn is_generator_function(function_def: &StmtFunctionDef) -> bool {
    for stmt in &function_def.body {
        if let Stmt::Expr(expr) = stmt {
            if let Expr::Yield(_) | Expr::YieldFrom(_) = *expr.value {
                return true;
            }
        }
    }
    false
}
