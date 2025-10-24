use std::path::{Path, PathBuf};

use karva_project::{project::Project, utils::module_name};
use pyo3::{prelude::*, types::PyModule};
use ruff_python_ast::{
    Expr, ModModule, PythonVersion, Stmt,
    visitor::source_order::{self, SourceOrderVisitor},
};
use ruff_python_parser::{Mode, ParseOptions, Parsed, parse_unchecked};

use crate::{
    diagnostic::Diagnostic,
    discovery::TestFunction,
    extensions::fixtures::{Fixture, is_fixture_function},
};

pub(crate) struct FunctionDefinitionVisitor<'proj, 'b> {
    discovered_functions: Vec<TestFunction>,
    fixture_definitions: Vec<Fixture>,
    project: &'proj Project,
    module_path: PathBuf,
    diagnostics: Vec<Diagnostic>,
    py_module: Bound<'b, PyModule>,
    py: Python<'b>,
    inside_function: bool,
    module_name: String,
}

impl<'proj, 'b> FunctionDefinitionVisitor<'proj, 'b> {
    pub(crate) fn new(
        py: Python<'b>,
        project: &'proj Project,
        module_path: &Path,
    ) -> Result<Self, String> {
        let module_name = module_name(project.cwd(), module_path)?;

        let py_module = py
            .import(&module_name)
            .map_err(|e| format!("Failed to import module {e}"))?;

        Ok(Self {
            discovered_functions: Vec::new(),
            fixture_definitions: Vec::new(),
            project,
            module_path: module_path.to_path_buf(),
            diagnostics: Vec::new(),
            py_module,
            inside_function: false,
            py,
            module_name,
        })
    }
}

impl SourceOrderVisitor<'_> for FunctionDefinitionVisitor<'_, '_> {
    fn visit_stmt(&mut self, stmt: &'_ Stmt) {
        if let Stmt::FunctionDef(function_def) = stmt {
            // Only consider top-level functions (not nested)
            if self.inside_function {
                return;
            }
            self.inside_function = true;
            if is_fixture_function(function_def) {
                let mut generator_function_visitor = GeneratorFunctionVisitor::default();

                source_order::walk_body(&mut generator_function_visitor, &function_def.body);

                let is_generator_function = generator_function_visitor.is_generator;

                match Fixture::try_from_function(
                    self.py,
                    function_def,
                    &self.py_module,
                    &self.module_name,
                    is_generator_function,
                ) {
                    Ok(Some(fixture_def)) => self.fixture_definitions.push(fixture_def),
                    Ok(None) => {}
                    Err(e) => {
                        self.diagnostics.push(Diagnostic::invalid_fixture(
                            Some(e),
                            Some(self.module_path.display().to_string()),
                        ));
                    }
                }
            } else if function_def
                .name
                .to_string()
                .starts_with(self.project.options().test_prefix())
            {
                if let Ok(py_function) = self.py_module.getattr(function_def.name.to_string()) {
                    self.discovered_functions.push(TestFunction::new(
                        self.module_name.clone(),
                        function_def.clone(),
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
pub(crate) struct DiscoveredFunctions {
    pub(crate) functions: Vec<TestFunction>,
    pub(crate) fixtures: Vec<Fixture>,
}

#[must_use]
pub(crate) fn discover(
    py: Python,
    path: &PathBuf,
    project: &Project,
) -> (DiscoveredFunctions, Vec<Diagnostic>) {
    let mut visitor = match FunctionDefinitionVisitor::new(py, project, path) {
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

    let parsed = parsed_module(path, project.metadata().python_version());
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
pub(crate) fn parsed_module(path: &PathBuf, python_version: PythonVersion) -> Parsed<ModModule> {
    let mode = Mode::Module;
    let options = ParseOptions::from(mode).with_target_version(python_version);
    let source = std::fs::read_to_string(path).expect("Failed to read source file");

    parse_unchecked(&source, options)
        .try_into_module()
        .expect("PySourceType always parses into a module")
}

#[derive(Default)]
pub(crate) struct GeneratorFunctionVisitor {
    is_generator: bool,
}

impl SourceOrderVisitor<'_> for GeneratorFunctionVisitor {
    fn visit_expr(&mut self, expr: &'_ Expr) {
        if let Expr::Yield(_) | Expr::YieldFrom(_) = *expr {
            self.is_generator = true;
        }
    }
}
