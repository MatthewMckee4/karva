use karva_project::{path::SystemPathBuf, project::Project, utils::module_name};
use pyo3::{prelude::*, types::PyModule};
use ruff_python_ast::{
    Expr, ModModule, PythonVersion, Stmt,
    visitor::source_order::{self, SourceOrderVisitor},
};
use ruff_python_parser::{Mode, ParseOptions, Parsed, parse_unchecked};

use crate::{
    diagnostic::Diagnostic,
    discovery::{DiscoveredModule, TestFunction},
    extensions::fixtures::{Fixture, is_fixture_function},
};

pub(crate) struct FunctionDefinitionVisitor<'proj, 'b> {
    discovered_functions: Vec<TestFunction>,
    fixture_definitions: Vec<Fixture>,
    project: &'proj Project,
    module_path: SystemPathBuf,
    diagnostics: Vec<Diagnostic>,
    py_module: Bound<'b, PyModule>,
    py: Python<'b>,
    inside_function: bool,
}

impl<'proj, 'b> FunctionDefinitionVisitor<'proj, 'b> {
    pub(crate) fn new(
        py: Python<'b>,
        project: &'proj Project,
        module_path: SystemPathBuf,
    ) -> Result<Self, String> {
        let module_name =
            module_name(project.cwd(), &module_path).ok_or("Failed to get module name")?;

        let py_module = py
            .import(module_name)
            .map_err(|_| "Failed to import module")?;

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
    py: Python<'_>,
    module: &DiscoveredModule,
    project: &Project,
) -> (DiscoveredFunctions, Vec<Diagnostic>) {
    let Ok(mut visitor) = FunctionDefinitionVisitor::new(py, project, module.path().clone()) else {
        return (
            DiscoveredFunctions {
                functions: Vec::new(),
                fixtures: Vec::new(),
            },
            vec![],
        );
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

#[must_use]
pub(crate) fn parsed_module(
    module: &DiscoveredModule,
    python_version: PythonVersion,
) -> Parsed<ModModule> {
    let mode = Mode::Module;
    let options = ParseOptions::from(mode).with_target_version(python_version);
    let source = module.source_text();

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
