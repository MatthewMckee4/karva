use std::path::Path;
use std::rc::Rc;

use camino::Utf8Path;
use fs_err as fs;
use karva_python_semantic::ModulePath;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use ruff_python_ast::visitor::source_order::{self, SourceOrderVisitor};
use ruff_python_ast::{Expr, PythonVersion, Stmt, StmtFunctionDef};
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
use ruff_source_file::SourceFileBuilder;

use crate::Context;
use crate::diagnostic::{
    report_failed_to_discover_imported_fixture, report_failed_to_import_module,
    report_generator_test, report_invalid_fixture,
};
use crate::discovery::{DiscoveredModule, DiscoveredTestFunction};
use crate::extensions::fixtures::DiscoveredFixture;
use crate::extensions::fixtures::python::FixtureFunctionDefinition;

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
    fn new(py: Python<'py>, context: &'ctx Context<'a>, module: &'b mut DiscoveredModule) -> Self {
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

        let is_generator_function = is_generator(&stmt_function_def);

        let stmt_function_def = Rc::new(stmt_function_def);

        match DiscoveredFixture::try_from_function(
            self.py,
            stmt_function_def.clone(),
            py_module,
            self.module.module_path(),
            self.module.source_file(),
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
            match DiscoveredTestFunction::new(
                self.py,
                self.module,
                Rc::new(stmt_function_def),
                py_function.unbind(),
            ) {
                Ok(test_function) => self.module.add_test_function(test_function),
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

    fn find_extra_fixtures(&mut self) {
        self.try_import_module();

        let Some(py_module) = self.py_module.clone() else {
            return;
        };

        for (name_obj, value) in py_module.dict().iter() {
            let Ok(name) = name_obj.extract::<String>() else {
                continue;
            };
            if value.is_callable() && is_fixture_value(&value) {
                self.try_process_imported_symbol(&py_module, &name);
            }
        }
    }

    fn try_process_imported_symbol(&mut self, py_module: &Bound<'_, PyModule>, name: &str) {
        let _ = self.resolve_imported_fixture(py_module, name);
    }

    /// Attempt to resolve an imported symbol as a fixture.
    ///
    /// Returns `None` at any step that fails — the symbol simply won't be
    /// discovered as a fixture.
    fn resolve_imported_fixture(
        &mut self,
        py_module: &Bound<'_, PyModule>,
        name: &str,
    ) -> Option<()> {
        let value = py_module.getattr(name).ok()?;

        if !value.is_callable() {
            return None;
        }

        if self
            .module
            .fixtures()
            .iter()
            .any(|f| f.name().function_name() == name)
        {
            return None;
        }

        if self
            .module
            .test_functions()
            .iter()
            .any(|f| f.name.function_name() == name)
        {
            return None;
        }

        let mut module_name = value.getattr("__module__").ok()?.extract::<String>().ok()?;

        if module_name == "builtins" {
            module_name = value
                .getattr("function")
                .ok()?
                .getattr("__module__")
                .ok()?
                .extract::<String>()
                .ok()?;
        }

        let imported_module = self.py.import(&module_name).ok()?;
        let file_name = imported_module
            .getattr("__file__")
            .ok()?
            .extract::<String>()
            .ok()?;
        let utf8_file_name = Utf8Path::from_path(Path::new(&file_name))?;
        let module_path = ModulePath::new(utf8_file_name, &self.context.cwd().to_path_buf())?;

        // Use the function's own __name__ to find its definition in the source, since the
        // conftest symbol name may differ when the fixture is imported under an alias.
        let func_name = value
            .getattr("__name__")
            .ok()
            .and_then(|n| n.extract::<String>().ok())
            .unwrap_or_else(|| name.to_string());

        let source_text = match fs::read_to_string(utf8_file_name) {
            Ok(source_text) => source_text,
            Err(err) => {
                report_failed_to_discover_imported_fixture(
                    self.context,
                    name,
                    self.module.source_file(),
                    utf8_file_name,
                    &err,
                );
                return None;
            }
        };

        let stmt_function_def =
            find_function_statement(&func_name, &source_text, self.context.python_version())?;

        let is_generator_function = is_generator(&stmt_function_def);
        let source_file = SourceFileBuilder::new(utf8_file_name.as_str(), source_text).finish();

        match DiscoveredFixture::try_from_function(
            self.py,
            stmt_function_def.clone(),
            &imported_module,
            &module_path,
            source_file.clone(),
            is_generator_function,
        ) {
            Ok(fixture_def) => self.module.add_fixture(fixture_def),
            Err(e) => {
                report_invalid_fixture(
                    self.context,
                    self.py,
                    source_file,
                    stmt_function_def.as_ref(),
                    &e,
                );
            }
        }

        Some(())
    }
}

pub fn discover(
    context: &Context,
    py: Python,
    module: &mut DiscoveredModule,
    test_function_defs: Vec<StmtFunctionDef>,
    fixture_function_defs: Vec<StmtFunctionDef>,
) {
    let is_conftest = module
        .path()
        .file_name()
        .is_some_and(|name| name == "conftest.py");

    let mut visitor = FunctionDefinitionVisitor::new(py, context, module);

    for test_function_def in test_function_defs {
        if is_generator(&test_function_def) {
            report_generator_test(context, visitor.module.source_file(), &test_function_def);
            continue;
        }

        visitor.process_test_function(test_function_def);
    }

    for fixture_function_def in fixture_function_defs {
        visitor.process_fixture_function(fixture_function_def);
    }

    if is_conftest || context.settings().test().try_import_fixtures {
        visitor.find_extra_fixtures();
    }
}

/// Returns `true` if the function body contains a yield or yield-from expression.
pub fn is_generator(stmt_function_def: &StmtFunctionDef) -> bool {
    let mut visitor = GeneratorFunctionVisitor::default();
    source_order::walk_body(&mut visitor, &stmt_function_def.body);
    visitor.is_generator
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
    fn visit_stmt(&mut self, stmt: &'_ Stmt) {
        match stmt {
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
            _ => source_order::walk_stmt(self, stmt),
        }
    }

    fn visit_expr(&mut self, expr: &'_ Expr) {
        if let Expr::Yield(_) | Expr::YieldFrom(_) = *expr {
            self.is_generator = true;
        } else {
            source_order::walk_expr(self, expr);
        }
    }
}

/// Returns `true` if `value` is a fixture — either a pytest-decorated function
/// (detected via `_fixture_function_marker` / `_pytestfixturefunction`) or a
/// Karva `FixtureFunctionDefinition` object.
fn is_fixture_value(value: &Bound<'_, PyAny>) -> bool {
    value.getattr("_fixture_function_marker").is_ok()
        || value.getattr("_pytestfixturefunction").is_ok()
        || value.cast::<FixtureFunctionDefinition>().is_ok()
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
