use std::path::Path;

use camino::Utf8Path;
use karva_project::project::Project;
use pyo3::{prelude::*, types::PyModule};
use ruff_python_ast::{
    Expr, ModModule, PythonVersion, Stmt, StmtFunctionDef,
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
    /// We only import the module once we actually need it, this ensures we don't import random files.
    /// Which has a side effect of running them.
    py_module: Option<Bound<'py, PyModule>>,
    py: Python<'py>,
    inside_function: bool,
    tried_to_import_module: bool,
}

impl<'proj, 'py, 'a> FunctionDefinitionVisitor<'proj, 'py, 'a> {
    const fn new(py: Python<'py>, project: &'proj Project, module: &'a DiscoveredModule) -> Self {
        Self {
            discovered_functions: Vec::new(),
            fixture_definitions: Vec::new(),
            project,
            module,
            diagnostics: Vec::new(),
            py_module: None,
            inside_function: false,
            py,
            tried_to_import_module: false,
        }
    }

    /// Try to import the current python module.
    ///
    /// If we have already tried to import the module, we don't try again.
    /// This ensures that we only first import the module when we need to.
    fn try_import_module(&mut self) {
        if self.tried_to_import_module {
            return;
        }

        self.tried_to_import_module = true;

        match self.py.import(self.module.name()) {
            Ok(py_module) => {
                self.py_module = Some(py_module);
            }
            Err(error) => {
                self.diagnostics.push(DiscoveryDiagnostic::failed_to_import(
                    self.module.name(),
                    &error.to_string(),
                ));
            }
        }
    }

    /// Sometimes users may import a fixture function that they want to use in the current module.
    ///
    /// This is not common and can add a lot of overhead so is disabled by default.
    /// Seen in <https://github.com/MatthewMckee4/karva/issues/253>
    fn find_extra_fixtures(&mut self) {
        self.try_import_module();

        let Some(py_module) = self.py_module.as_ref() else {
            return;
        };

        let module_dict = py_module.dict();

        'outer: for (name, value) in module_dict.iter() {
            if value.is_callable() {
                let name_str = name.extract::<String>().unwrap_or_default();

                for fixture in &self.fixture_definitions {
                    if fixture.original_function_name() == name_str {
                        continue 'outer;
                    }
                }

                for function in &self.discovered_functions {
                    if function.name().function_name() == name_str {
                        continue 'outer;
                    }
                }

                let Ok(module_name) = value.getattr("__module__") else {
                    continue;
                };

                let Ok(mut module_name) = module_name.extract::<String>() else {
                    continue;
                };

                if module_name == "builtins" {
                    let Ok(function) = value.getattr("function") else {
                        continue;
                    };

                    let Ok(function_module_name) = function.getattr("__module__") else {
                        continue;
                    };

                    if let Ok(actual_module_name) = function_module_name.extract::<String>() {
                        module_name = actual_module_name;
                    } else {
                        continue;
                    }
                }

                let py_module = match self.py.import(&module_name) {
                    Ok(py_module) => py_module,
                    Err(error) => {
                        self.diagnostics.push(DiscoveryDiagnostic::failed_to_import(
                            &module_name,
                            &error.to_string(),
                        ));
                        continue;
                    }
                };

                let Ok(file_name) = py_module.getattr("__file__") else {
                    continue;
                };

                let Ok(file_name) = file_name.extract::<String>() else {
                    continue;
                };
                let std_path = Path::new(&file_name);

                let Some(utf8_file_name) = Utf8Path::from_path(std_path) else {
                    continue;
                };

                let Ok(source_text) = std::fs::read_to_string(utf8_file_name) else {
                    continue;
                };

                let parsed = parsed_module(&source_text, self.project.metadata().python_version());

                let mut visitor = FindFunctionVisitor::new(name_str);

                visitor.visit_body(&parsed.syntax().body);

                let Some(stmt_function_def) = visitor.function_def else {
                    continue;
                };

                let mut generator_function_visitor = GeneratorFunctionVisitor::default();

                source_order::walk_body(&mut generator_function_visitor, &stmt_function_def.body);

                let is_generator_function = generator_function_visitor.is_generator;

                if let Ok(fixture_def) = Fixture::try_from_function(
                    self.py,
                    stmt_function_def,
                    &py_module,
                    self.module.module_path(),
                    is_generator_function,
                ) {
                    self.fixture_definitions.push(fixture_def);
                }
            }
        }
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
                self.try_import_module();

                let Some(py_module) = self.py_module.as_ref() else {
                    return;
                };

                let mut generator_function_visitor = GeneratorFunctionVisitor::default();

                source_order::walk_body(&mut generator_function_visitor, &stmt_function_def.body);

                let is_generator_function = generator_function_visitor.is_generator;

                match Fixture::try_from_function(
                    self.py,
                    stmt_function_def,
                    py_module,
                    self.module.module_path(),
                    is_generator_function,
                ) {
                    Ok(fixture_def) => self.fixture_definitions.push(fixture_def),
                    Err(e) => {
                        self.diagnostics.push(DiscoveryDiagnostic::invalid_fixture(
                            e,
                            Some(function_definition_location(
                                self.project.cwd(),
                                self.module,
                                stmt_function_def,
                            )),
                            stmt_function_def.name.to_string(),
                        ));
                    }
                }
            } else if stmt_function_def
                .name
                .to_string()
                .starts_with(self.project.options().test_prefix())
            {
                self.try_import_module();

                let Some(py_module) = self.py_module.as_ref() else {
                    return;
                };

                if let Ok(py_function) = py_module.getattr(stmt_function_def.name.to_string()) {
                    self.discovered_functions.push(TestFunction::new(
                        self.py,
                        self.module.module_path().clone(),
                        stmt_function_def.clone(),
                        py_function.unbind(),
                    ));
                }
            }

            self.inside_function = false;
        }

        source_order::walk_stmt(self, stmt);
    }
}

#[derive(Debug, Default)]
pub struct DiscoveredFunctions {
    pub(crate) functions: Vec<TestFunction>,
    pub(crate) fixtures: Vec<Fixture>,
}

pub fn discover(
    py: Python,
    module: &DiscoveredModule,
    project: &Project,
) -> (DiscoveredFunctions, Vec<DiscoveryDiagnostic>) {
    tracing::info!(
        "Discovering test functions and fixtures in module {}",
        module.name()
    );

    let mut visitor = FunctionDefinitionVisitor::new(py, project, module);

    let parsed = parsed_module(module.source_text(), project.metadata().python_version());

    visitor.visit_body(&parsed.syntax().body);

    if project.options().try_import_fixtures() {
        visitor.find_extra_fixtures();
    }

    (
        DiscoveredFunctions {
            functions: visitor.discovered_functions,
            fixtures: visitor.fixture_definitions,
        },
        visitor.diagnostics,
    )
}

fn parsed_module(source: &str, python_version: PythonVersion) -> Parsed<ModModule> {
    let mode = Mode::Module;
    let options = ParseOptions::from(mode).with_target_version(python_version);

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

struct FindFunctionVisitor<'ast> {
    name: String,
    function_def: Option<&'ast StmtFunctionDef>,
}

impl FindFunctionVisitor<'_> {
    const fn new(name: String) -> Self {
        Self {
            name,
            function_def: None,
        }
    }
}

impl<'ast> SourceOrderVisitor<'ast> for FindFunctionVisitor<'ast> {
    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if let Stmt::FunctionDef(function_def) = stmt {
            if function_def.name.as_str() == self.name {
                self.function_def = Some(function_def);
            }
        }
    }
}
