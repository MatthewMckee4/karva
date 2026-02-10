use std::path::Path;
use std::rc::Rc;

use camino::Utf8Path;
use karva_python_semantic::{ModulePath, is_fixture_function};
use pyo3::prelude::*;
use pyo3::types::PyModule;
use ruff_python_ast::visitor::source_order::{self, SourceOrderVisitor};
use ruff_python_ast::{Expr, PythonVersion, Stmt, StmtFunctionDef};
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
use ruff_source_file::SourceFileBuilder;

use crate::Context;
use crate::diagnostic::{report_failed_to_import_module, report_invalid_fixture};
use crate::discovery::{DiscoveredModule, DiscoveredTestFunction};
use crate::extensions::fixtures::DiscoveredFixture;

/// Visitor for discovering test functions and fixture definitions in a given module.
///
/// Processes function definitions found during AST traversal and converts them
/// into test functions or fixtures by importing the corresponding Python module.
struct FunctionDefinitionVisitor<'ctx, 'py, 'a, 'b> {
    /// Reference to the test execution context.
    context: &'ctx Context<'a>,

    /// The module being populated with discovered test functions and fixtures.
    module: &'b mut DiscoveredModule,

    /// Lazily-loaded Python module, imported only when needed to avoid side effects.
    py_module: Option<Bound<'py, PyModule>>,

    /// Python interpreter handle for this visitor.
    py: Python<'py>,

    /// Flag to prevent multiple import attempts for the same module.
    tried_to_import_module: bool,
}

impl<'ctx, 'py, 'a, 'b> FunctionDefinitionVisitor<'ctx, 'py, 'a, 'b> {
    const fn new(
        py: Python<'py>,
        context: &'ctx Context<'a>,
        module: &'b mut DiscoveredModule,
    ) -> Self {
        Self {
            context,
            module,
            py_module: None,
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
                report_failed_to_import_module(
                    self.context,
                    self.module.name(),
                    &error.value(self.py).to_string(),
                );
            }
        }
    }
}

impl FunctionDefinitionVisitor<'_, '_, '_, '_> {
    fn process_fixture_function(&mut self, stmt_function_def: StmtFunctionDef) {
        self.try_import_module();

        let Some(py_module) = self.py_module.as_ref() else {
            return;
        };

        let mut generator_function_visitor = GeneratorFunctionVisitor::default();

        source_order::walk_body(&mut generator_function_visitor, &stmt_function_def.body);

        let is_generator_function = generator_function_visitor.is_generator;

        let stmt_function_def = Rc::new(stmt_function_def);

        match DiscoveredFixture::try_from_function(
            self.py,
            stmt_function_def.clone(),
            py_module,
            self.module.module_path(),
            is_generator_function,
        ) {
            Ok(fixture_def) => self.module.add_fixture(fixture_def),
            Err(e) => {
                report_invalid_fixture(
                    self.context,
                    self.py,
                    self.module.source_file(),
                    &stmt_function_def,
                    &e,
                );
            }
        }
    }

    fn process_test_function(&mut self, stmt_function_def: StmtFunctionDef) {
        self.try_import_module();

        let Some(py_module) = self.py_module.as_ref() else {
            return;
        };

        if let Ok(py_function) = py_module.getattr(stmt_function_def.name.to_string()) {
            self.module.add_test_function(DiscoveredTestFunction::new(
                self.py,
                self.module,
                Rc::new(stmt_function_def),
                py_function.unbind(),
            ));
        }
    }

    fn find_extra_fixtures(&mut self) {
        self.try_import_module();

        let Some(py_module) = self.py_module.clone() else {
            return;
        };

        let symbols =
            find_imported_symbols(self.module.source_text(), self.context.python_version());

        for ImportedSymbol { name } in symbols {
            self.try_process_imported_symbol(&py_module, &name);
        }
    }

    fn try_process_imported_symbol(&mut self, py_module: &Bound<'_, PyModule>, name: &str) {
        let Ok(value) = py_module.getattr(name) else {
            return;
        };

        if !value.is_callable() {
            return;
        }

        if self
            .module
            .fixtures()
            .iter()
            .any(|f| f.name().function_name() == name)
        {
            return;
        }

        if self
            .module
            .test_functions()
            .iter()
            .any(|f| f.name.function_name() == name)
        {
            return;
        }

        let Ok(module_name_attr) = value.getattr("__module__") else {
            return;
        };

        let Ok(mut module_name) = module_name_attr.extract::<String>() else {
            return;
        };

        if module_name == "builtins" {
            let Ok(function) = value.getattr("function") else {
                return;
            };

            let Ok(function_module_name) = function.getattr("__module__") else {
                return;
            };

            let Ok(actual_module_name) = function_module_name.extract::<String>() else {
                return;
            };

            module_name = actual_module_name;
        }

        let Ok(imported_module) = self.py.import(&module_name) else {
            return;
        };

        let Ok(file_name) = imported_module.getattr("__file__") else {
            return;
        };

        let Ok(file_name) = file_name.extract::<String>() else {
            return;
        };

        let std_path = Path::new(&file_name);

        let Some(utf8_file_name) = Utf8Path::from_path(std_path) else {
            return;
        };

        let Some(module_path) = ModulePath::new(utf8_file_name, &self.context.cwd().to_path_buf())
        else {
            return;
        };

        let Ok(source_text) = std::fs::read_to_string(utf8_file_name) else {
            return;
        };

        let Some(stmt_function_def) =
            find_function_statement(name, &source_text, self.context.python_version())
        else {
            return;
        };

        if !is_fixture_function(&stmt_function_def) {
            return;
        }

        let mut generator_function_visitor = GeneratorFunctionVisitor::default();

        source_order::walk_body(&mut generator_function_visitor, &stmt_function_def.body);

        let is_generator_function = generator_function_visitor.is_generator;

        match DiscoveredFixture::try_from_function(
            self.py,
            stmt_function_def.clone(),
            &imported_module,
            &module_path,
            is_generator_function,
        ) {
            Ok(fixture_def) => self.module.add_fixture(fixture_def),
            Err(e) => {
                report_invalid_fixture(
                    self.context,
                    self.py,
                    SourceFileBuilder::new(utf8_file_name.as_str(), source_text).finish(),
                    stmt_function_def.as_ref(),
                    &e,
                );
            }
        }
    }
}

pub fn discover(
    context: &Context,
    py: Python,
    module: &mut DiscoveredModule,
    test_function_defs: Vec<StmtFunctionDef>,
    fixture_function_defs: Vec<StmtFunctionDef>,
) {
    let mut visitor = FunctionDefinitionVisitor::new(py, context, module);

    for test_function_def in test_function_defs {
        visitor.process_test_function(test_function_def);
    }

    for fixture_function_def in fixture_function_defs {
        visitor.process_fixture_function(fixture_function_def);
    }

    if context.settings().test().try_import_fixtures {
        visitor.find_extra_fixtures();
    }
}

/// Visitor that detects whether a function contains yield expressions.
///
/// Used to identify generator functions, which is important for fixture
/// finalization behavior.
#[derive(Default)]
struct GeneratorFunctionVisitor {
    /// Set to true if a yield or yield-from expression is found.
    is_generator: bool,
}

impl SourceOrderVisitor<'_> for GeneratorFunctionVisitor {
    fn visit_expr(&mut self, expr: &'_ Expr) {
        if let Expr::Yield(_) | Expr::YieldFrom(_) = *expr {
            self.is_generator = true;
        }
    }
}

fn find_function_statement(
    name: &str,
    source_text: &str,
    python_version: PythonVersion,
) -> Option<Rc<StmtFunctionDef>> {
    let mut parse_options = ParseOptions::from(Mode::Module);

    parse_options = parse_options.with_target_version(python_version);

    let parsed = parse_unchecked(source_text, parse_options).try_into_module()?;

    for stmt in parsed.into_syntax().body {
        if let Stmt::FunctionDef(function_def) = stmt {
            if function_def.name.as_str() == name {
                return Some(Rc::new(function_def));
            }
        }
    }

    None
}

/// Represents a symbol imported into a module via `from ... import ...`.
///
/// Used to track imported fixtures that may need to be discovered.
struct ImportedSymbol {
    /// The name of the imported symbol.
    name: String,
}

fn find_imported_symbols(source_text: &str, python_version: PythonVersion) -> Vec<ImportedSymbol> {
    let mut parse_options = ParseOptions::from(Mode::Module);

    parse_options = parse_options.with_target_version(python_version);

    let mut symbols = Vec::new();

    let Some(parsed) = parse_unchecked(source_text, parse_options).try_into_module() else {
        return symbols;
    };

    for stmt in parsed.into_syntax().body {
        if let Stmt::ImportFrom(stmt_import_from) = stmt {
            for name in stmt_import_from.names {
                if name.asname.is_some() {
                    continue;
                }
                symbols.push(ImportedSymbol {
                    name: name.name.to_string(),
                });
            }
        }
    }

    symbols
}
