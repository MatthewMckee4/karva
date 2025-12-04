use std::sync::Arc;

use pyo3::{prelude::*, types::PyModule};
use ruff_python_ast::{
    Expr, StmtFunctionDef,
    visitor::source_order::{self, SourceOrderVisitor},
};

use crate::{
    Context,
    diagnostic::{report_failed_to_import_module, report_invalid_fixture},
    discovery::{DiscoveredModule, TestFunction},
    extensions::fixtures::Fixture,
};

/// Visitor for discovering test functions and fixture definitions in a given module.
struct FunctionDefinitionVisitor<'ctx, 'proj, 'rep, 'py, 'a> {
    /// Context for the current session.
    context: &'ctx Context<'proj, 'rep>,

    /// The current module.
    module: &'a mut DiscoveredModule,

    /// We only import the module once we actually need it, this ensures we don't import random files.
    /// Which has a side effect of running them.
    py_module: Option<Bound<'py, PyModule>>,

    py: Python<'py>,

    /// Used to track whether we have tried to import the current module yet.
    tried_to_import_module: bool,
}

impl<'ctx, 'proj, 'rep, 'py, 'a> FunctionDefinitionVisitor<'ctx, 'proj, 'rep, 'py, 'a> {
    const fn new(
        py: Python<'py>,
        context: &'ctx Context<'proj, 'rep>,
        module: &'a mut DiscoveredModule,
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

impl FunctionDefinitionVisitor<'_, '_, '_, '_, '_> {
    fn process_fixture_function(&mut self, stmt_function_def: &Arc<StmtFunctionDef>) {
        self.try_import_module();

        let Some(py_module) = self.py_module.as_ref() else {
            return;
        };

        let mut generator_function_visitor = GeneratorFunctionVisitor::default();

        source_order::walk_body(&mut generator_function_visitor, &stmt_function_def.body);

        let is_generator_function = generator_function_visitor.is_generator;

        match Fixture::try_from_function(
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
                    stmt_function_def,
                    &e,
                );
            }
        }
    }

    fn process_test_function(&mut self, stmt_function_def: Arc<StmtFunctionDef>) {
        self.try_import_module();

        let Some(py_module) = self.py_module.as_ref() else {
            return;
        };

        if let Ok(py_function) = py_module.getattr(stmt_function_def.name.to_string()) {
            self.module.add_test_function(TestFunction::new(
                self.py,
                self.module,
                stmt_function_def,
                py_function.unbind(),
            ));
        }
    }
}

pub fn discover(
    context: &Context,
    py: Python,
    module: &mut DiscoveredModule,
    test_function_defs: Vec<Arc<StmtFunctionDef>>,
    fixture_function_defs: Vec<Arc<StmtFunctionDef>>,
) {
    tracing::info!(
        "Discovering test functions and fixtures in module {}",
        module.name()
    );

    let mut visitor = FunctionDefinitionVisitor::new(py, context, module);

    for test_function_def in test_function_defs {
        visitor.process_test_function(test_function_def);
    }

    for fixture_function_def in fixture_function_defs {
        visitor.process_fixture_function(&fixture_function_def);
    }
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
