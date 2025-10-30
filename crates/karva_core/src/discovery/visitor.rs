use karva_project::project::Project;
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
    name::ModulePath,
};

pub(crate) struct FunctionDefinitionVisitor<'proj, 'py, 'a> {
    discovered_functions: Vec<TestFunction>,
    fixture_definitions: Vec<Fixture>,
    project: &'proj Project,
    module_path: &'a ModulePath,
    diagnostics: Vec<Diagnostic>,
    py_module: Bound<'py, PyModule>,
    py: Python<'py>,
    inside_function: bool,
}

impl<'proj, 'py, 'a> FunctionDefinitionVisitor<'proj, 'py, 'a> {
    pub(crate) fn new(
        py: Python<'py>,
        project: &'proj Project,
        module_path: &'a ModulePath,
    ) -> Result<Self, String> {
        let py_module = py
            .import(module_path.module_name())
            .map_err(|e| format!("Failed to import module {e}"))?;

        Ok(Self {
            discovered_functions: Vec::new(),
            fixture_definitions: Vec::new(),
            project,
            module_path,
            diagnostics: Vec::new(),
            py_module,
            inside_function: false,
            py,
        })
    }
}

impl SourceOrderVisitor<'_> for FunctionDefinitionVisitor<'_, '_, '_> {
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
                    self.module_path,
                    is_generator_function,
                ) {
                    Ok(Some(fixture_def)) => self.fixture_definitions.push(fixture_def),
                    Ok(None) => {}
                    Err(e) => {
                        self.diagnostics.push(Diagnostic::invalid_fixture(
                            Some(e),
                            Some(self.module_path.module_name().to_string()),
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
                        self.module_path.clone(),
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
    module_path: &ModulePath,
    project: &Project,
) -> (DiscoveredFunctions, Vec<Diagnostic>) {
    let mut visitor = match FunctionDefinitionVisitor::new(py, project, module_path) {
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

    let parsed = parsed_module(module_path, project.metadata().python_version());
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
pub(crate) fn parsed_module(
    module_path: &ModulePath,
    python_version: PythonVersion,
) -> Parsed<ModModule> {
    let mode = Mode::Module;
    let options = ParseOptions::from(mode).with_target_version(python_version);
    let source =
        std::fs::read_to_string(module_path.module_path()).expect("Failed to read source file");

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
