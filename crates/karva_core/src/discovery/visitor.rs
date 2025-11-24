use karva_project::project::Project;
use pyo3::{prelude::*, types::PyModule};
use ruff_python_ast::{
    Expr, ModModule, PythonVersion, Stmt,
    visitor::source_order::{self, SourceOrderVisitor},
};
use ruff_python_parser::{Mode, ParseOptions, Parsed, parse_unchecked};

use crate::{
    diagnostic::DiscoveryDiagnostic,
    discovery::{DiscoveredModule, TestFunction},
    extensions::fixtures::{Fixture, is_fixture_function},
    utils::function_definition_location,
};

struct FunctionDefinitionVisitor<'proj, 'py, 'a> {
    discovered_functions: Vec<TestFunction>,
    fixture_definitions: Vec<Fixture>,
    project: &'proj Project,
    module: &'a DiscoveredModule,
    diagnostics: Vec<DiscoveryDiagnostic>,
    py_module: Bound<'py, PyModule>,
    py: Python<'py>,
    inside_function: bool,
}

impl<'proj, 'py, 'a> FunctionDefinitionVisitor<'proj, 'py, 'a> {
    pub fn new(
        py: Python<'py>,
        project: &'proj Project,
        module: &'a DiscoveredModule,
    ) -> Result<Self, String> {
        let py_module = py
            .import(module.name())
            .map_err(|e| format!("Failed to import module {e}"))?;

        Ok(Self {
            discovered_functions: Vec::new(),
            fixture_definitions: Vec::new(),
            project,
            module,
            diagnostics: Vec::new(),
            py_module,
            inside_function: false,
            py,
        })
    }
}

impl SourceOrderVisitor<'_> for FunctionDefinitionVisitor<'_, '_, '_> {
    fn visit_stmt(&mut self, stmt: &'_ Stmt) {
        if let Stmt::FunctionDef(stmt_function_def) = stmt {
            // Only consider top-level functions (not nested)
            if self.inside_function {
                return;
            }
            self.inside_function = true;
            if is_fixture_function(stmt_function_def) {
                let mut generator_function_visitor = GeneratorFunctionVisitor::default();

                source_order::walk_body(&mut generator_function_visitor, &stmt_function_def.body);

                let is_generator_function = generator_function_visitor.is_generator;

                match Fixture::try_from_function(
                    self.py,
                    stmt_function_def,
                    &self.py_module,
                    self.module.module_path(),
                    is_generator_function,
                ) {
                    Ok(Some(fixture_def)) => self.fixture_definitions.push(fixture_def),
                    Ok(None) => {}
                    Err(e) => {
                        self.diagnostics.push(DiscoveryDiagnostic::invalid_fixture(
                            e,
                            function_definition_location(self.module, stmt_function_def),
                            stmt_function_def.name.to_string(),
                        ));
                    }
                }
            } else if stmt_function_def
                .name
                .to_string()
                .starts_with(self.project.options().test_prefix())
            {
                if let Ok(py_function) = self.py_module.getattr(stmt_function_def.name.to_string())
                {
                    self.discovered_functions.push(TestFunction::new(
                        self.py,
                        self.module.module_path().clone(),
                        stmt_function_def.clone(),
                        py_function.unbind(),
                    ));
                }
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
    pub(crate) functions: Vec<TestFunction>,
    pub(crate) fixtures: Vec<Fixture>,
}

pub fn discover(
    py: Python,
    module: &DiscoveredModule,
    project: &Project,
) -> (DiscoveredFunctions, Vec<DiscoveryDiagnostic>) {
    let mut visitor = match FunctionDefinitionVisitor::new(py, project, module) {
        Ok(visitor) => visitor,
        Err(e) => {
            tracing::debug!("Failed to create discovery module: {e}");
            return (
                DiscoveredFunctions {
                    functions: Vec::new(),
                    fixtures: Vec::new(),
                },
                vec![],
            );
        }
    };

    let parsed = parsed_module(module, project.metadata().python_version());
    visitor.visit_body(&parsed.syntax().body);

    (
        DiscoveredFunctions {
            functions: visitor.discovered_functions,
            fixtures: visitor.fixture_definitions,
        },
        visitor.diagnostics,
    )
}

fn parsed_module(module: &DiscoveredModule, python_version: PythonVersion) -> Parsed<ModModule> {
    let mode = Mode::Module;
    let options = ParseOptions::from(mode).with_target_version(python_version);
    let source = module.source_text();

    parse_unchecked(source, options)
        .try_into_module()
        .expect("PySourceType always parses into a module")
}

#[derive(Default)]
struct GeneratorFunctionVisitor {
    is_generator: bool,
}

impl SourceOrderVisitor<'_> for GeneratorFunctionVisitor {
    fn visit_expr(&mut self, expr: &'_ Expr) {
        if let Expr::Yield(_) | Expr::YieldFrom(_) = *expr {
            self.is_generator = true;
        }
    }
}
