use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;

use camino::Utf8Path;
use fs_err as fs;
use karva_collector::{CollectedDoctest, DoctestTarget};
use karva_python_semantic::ModulePath;
use pyo3::prelude::*;
use pyo3::types::PyModule;
use ruff_python_ast::visitor::source_order::{self, SourceOrderVisitor};
use ruff_python_ast::{Expr, PythonVersion, Stmt, StmtFunctionDef};
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
use ruff_source_file::SourceFileBuilder;
use ruff_text_size::TextRange;

use crate::Context;
use crate::discovery::{DiscoveredModule, DiscoveredTestFunction, DiscoveryError, DiscoveryIssue};
use crate::extensions::fixtures::python::FixtureFunctionDefinition;
use crate::extensions::fixtures::{DiscoveredFixture, RejectedFixture};
use crate::extensions::tags::skip::{extract_skip_reason, is_skip_exception};
use crate::extensions::tags::validation::unknown_runtime_tags;

/// Visitor for discovering executable tests and fixture definitions in a given module.
///
/// Resolves collected source definitions and doctest metadata against the imported Python module.
struct FunctionDefinitionVisitor<'ctx, 'py, 'a, 'b> {
    /// Reference to the test execution context.
    context: &'ctx Context<'a>,

    /// The module being populated with discovered test functions and fixtures.
    module: &'b mut DiscoveredModule,

    /// Complete module statements used to locate runtime-discovered module marks during strict
    /// tag validation. Empty when strict tags are disabled.
    module_body: Box<[Stmt]>,

    /// Lazily-loaded Python module, imported only when needed to avoid side effects.
    py_module: Option<Bound<'py, PyModule>>,

    /// Python interpreter handle for this visitor.
    py: Python<'py>,

    /// Flag to prevent multiple import attempts for the same module.
    tried_to_import_module: bool,

    /// Issues produced while discovering this module, in source order.
    issues: Vec<DiscoveryIssue>,
}

impl<'ctx, 'py, 'a, 'b> FunctionDefinitionVisitor<'ctx, 'py, 'a, 'b> {
    fn new(
        py: Python<'py>,
        context: &'ctx Context<'a>,
        module: &'b mut DiscoveredModule,
        module_body: Box<[Stmt]>,
    ) -> Self {
        Self {
            context,
            module,
            module_body,
            py_module: None,
            py,
            tried_to_import_module: false,
            issues: Vec::new(),
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
                if is_skip_exception(self.py, &error) {
                    self.issues.push(DiscoveryIssue::SkippedModule {
                        module_path: self.module.module_path().clone(),
                        reason: extract_skip_reason(self.py, &error),
                    });
                } else {
                    self.issues
                        .push(DiscoveryIssue::Error(DiscoveryError::Import {
                            module_name: self.module.name().to_string(),
                            reason: error.value(self.py).to_string(),
                        }));
                }
            }
        }
    }
}

impl FunctionDefinitionVisitor<'_, '_, '_, '_> {
    fn process_fixture_function(
        &mut self,
        stmt_function_def: StmtFunctionDef,
    ) -> Option<DiscoveredFixture> {
        self.try_import_module();

        let py_module = self.py_module.as_ref()?;

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
            Ok(fixture_def) => Some(fixture_def),
            Err(error) => {
                let source_file = self.module.source_file();
                let exposure_name =
                    DiscoveredFixture::exposure_name_from_function(&stmt_function_def, py_module);
                self.module.add_rejected_fixture(RejectedFixture::new(
                    exposure_name,
                    error.value(self.py).to_string(),
                    Rc::clone(&stmt_function_def),
                    source_file,
                    self.module.module_path().clone(),
                ));
                self.issues
                    .push(DiscoveryIssue::Error(DiscoveryError::InvalidFixture {
                        source_file: self.module.source_file(),
                        definition: stmt_function_def,
                        reason: error.value(self.py).to_string(),
                    }));
                None
            }
        }
    }

    fn process_test_function(
        &mut self,
        stmt_function_def: StmtFunctionDef,
        case_filter: Option<Vec<usize>>,
    ) {
        self.try_import_module();

        let Some(py_module) = self.py_module.as_ref() else {
            return;
        };

        if let Ok(py_function) = py_module.getattr(stmt_function_def.name.to_string()) {
            match DiscoveredTestFunction::new_function(
                self.py,
                self.module,
                py_module,
                Rc::new(stmt_function_def),
                py_function.unbind(),
                case_filter,
            ) {
                Ok(test_function) => {
                    if self.context.settings().test().strict_tags {
                        let unknown = unknown_runtime_tags(
                            test_function.function_statement(),
                            test_function.diagnostic_range(),
                            &self.module_body,
                            &test_function.tags,
                            self.context.settings().tags(),
                        );
                        if !unknown.is_empty() {
                            for unknown in unknown {
                                self.issues.push(DiscoveryIssue::Error(
                                    DiscoveryError::UnknownTag {
                                        source_file: self.module.source_file(),
                                        name: unknown.name,
                                        range: unknown.range,
                                        suggestion: unknown.suggestion,
                                    },
                                ));
                            }
                            return;
                        }
                    }
                    self.module.add_test_function(test_function);
                }
                Err(error) => {
                    self.issues
                        .push(DiscoveryIssue::Error(DiscoveryError::Import {
                            module_name: self.module.name().to_string(),
                            reason: error.value(self.py).to_string(),
                        }));
                }
            }
        }
    }

    fn process_doctests(&mut self, doctests: Vec<CollectedDoctest>) {
        if doctests.is_empty() {
            return;
        }

        self.try_import_module();
        let Some(py_module) = self.py_module.clone() else {
            return;
        };
        let functions = match crate::doctest::find_doctest_functions(self.py, &py_module) {
            Ok(functions) => functions,
            Err(error) => {
                self.issues
                    .push(DiscoveryIssue::Error(DiscoveryError::Import {
                        module_name: self.module.name().to_string(),
                        reason: error.value(self.py).to_string(),
                    }));
                return;
            }
        };

        for doctest in doctests {
            let object_name = match &doctest.target {
                DoctestTarget::Module => self.module.name().to_string(),
                DoctestTarget::Object(object_name) => {
                    format!("{}.{}", self.module.name(), object_name)
                }
            };
            let (function, missing_reason) = match functions.get_item(&object_name) {
                Ok(Some(function)) => (function, None),
                Ok(None) => {
                    let reason =
                        format!("Doctest `{object_name}` is not available after module import");
                    match crate::doctest::missing_doctest_function(self.py, &reason) {
                        Ok(function) => (function, Some(reason)),
                        Err(error) => {
                            self.issues
                                .push(DiscoveryIssue::Error(DiscoveryError::Import {
                                    module_name: self.module.name().to_string(),
                                    reason: error.value(self.py).to_string(),
                                }));
                            continue;
                        }
                    }
                }
                Err(error) => {
                    self.issues
                        .push(DiscoveryIssue::Error(DiscoveryError::Import {
                            module_name: self.module.name().to_string(),
                            reason: error.value(self.py).to_string(),
                        }));
                    continue;
                }
            };
            match DiscoveredTestFunction::new_doctest(
                self.py,
                self.module,
                &py_module,
                doctest.name,
                doctest.range,
                function.unbind(),
            ) {
                Ok(mut test_function) => {
                    test_function.tags.remove_parametrize();
                    if let Some(reason) = missing_reason {
                        test_function.tags.add_skip(reason);
                    }
                    if self.context.settings().test().strict_tags {
                        let unknown = unknown_runtime_tags(
                            test_function.function_statement(),
                            test_function.diagnostic_range(),
                            &self.module_body,
                            &test_function.tags,
                            self.context.settings().tags(),
                        );
                        if !unknown.is_empty() {
                            for unknown in unknown {
                                self.issues.push(DiscoveryIssue::Error(
                                    DiscoveryError::UnknownTag {
                                        source_file: self.module.source_file(),
                                        name: unknown.name,
                                        range: unknown.range,
                                        suggestion: unknown.suggestion,
                                    },
                                ));
                            }
                            continue;
                        }
                    }
                    self.module.add_test_function(test_function);
                }
                Err(error) => {
                    self.issues
                        .push(DiscoveryIssue::Error(DiscoveryError::Import {
                            module_name: self.module.name().to_string(),
                            reason: error.value(self.py).to_string(),
                        }));
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
            .any(|f| f.name().function_name() == name)
        {
            return None;
        }

        if self.module.rejected_fixture_symbol(name).is_some() {
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
        let module_path = ModulePath::new(utf8_file_name, self.context.cwd())?;

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
                self.issues
                    .push(DiscoveryIssue::Error(DiscoveryError::ImportedFixture {
                        fixture_name: name.to_string(),
                        source_path: utf8_file_name.to_path_buf(),
                        error: err,
                    }));
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
            Err(error) => {
                // Imported fixtures are exposed under their conftest binding.
                self.module.add_rejected_fixture(RejectedFixture::new(
                    name.to_string(),
                    error.value(self.py).to_string(),
                    Rc::clone(&stmt_function_def),
                    source_file.clone(),
                    module_path.clone(),
                ));
                self.issues
                    .push(DiscoveryIssue::Error(DiscoveryError::InvalidFixture {
                        source_file,
                        definition: stmt_function_def,
                        reason: error.value(self.py).to_string(),
                    }));
            }
        }

        Some(())
    }
}

/// Binds collected AST definitions to imported Python callables.
///
/// Duplicate or invalid definitions emit diagnostics and are excluded. Imported fixtures
/// are scanned only for `conftest.py` or when `try_import_fixtures` is enabled. In strict-tag mode,
/// the complete module body provides source ranges for marks inherited by each test at runtime.
pub fn discover(
    context: &Context,
    py: Python,
    module: &mut DiscoveredModule,
    module_body: Box<[Stmt]>,
    test_function_defs: Vec<(StmtFunctionDef, Option<Vec<usize>>)>,
    doctests: Vec<CollectedDoctest>,
    fixture_function_defs: Vec<StmtFunctionDef>,
) -> Vec<DiscoveryIssue> {
    let is_conftest = module
        .path()
        .file_name()
        .is_some_and(|name| name == "conftest.py");

    let mut visitor = FunctionDefinitionVisitor::new(py, context, module, module_body);

    let duplicate_test_indices = duplicate_definition_indices(
        &test_function_defs,
        |(test_function_def, _)| test_function_def.name.to_string(),
        |(test_function_def, _)| test_function_def.range,
        |name, first_definition, duplicate_definition| {
            visitor
                .issues
                .push(DiscoveryIssue::Error(DiscoveryError::DuplicateTest {
                    source_file: visitor.module.source_file(),
                    test_name: name.to_string(),
                    first_definition: Rc::new(first_definition.0.clone()),
                    duplicate_definition: Rc::new(duplicate_definition.0.clone()),
                }));
        },
    );

    for (index, (test_function_def, case_filter)) in test_function_defs.into_iter().enumerate() {
        if duplicate_test_indices.contains(&index) {
            continue;
        }

        if is_generator(&test_function_def) {
            visitor
                .issues
                .push(DiscoveryIssue::Error(DiscoveryError::GeneratorTest {
                    source_file: visitor.module.source_file(),
                    definition: Rc::new(test_function_def),
                }));
            continue;
        }

        visitor.process_test_function(test_function_def, case_filter);
    }

    visitor.process_doctests(doctests);

    let mut fixtures = Vec::with_capacity(fixture_function_defs.len());
    for fixture_function_def in fixture_function_defs {
        if let Some(fixture) = visitor.process_fixture_function(fixture_function_def) {
            fixtures.push(fixture);
        }
    }

    let duplicate_fixture_indices = duplicate_definition_indices(
        &fixtures,
        |fixture| fixture.name().function_name().to_string(),
        |fixture| fixture.stmt_function_def().range,
        |name, first_definition, duplicate_definition| {
            visitor
                .issues
                .push(DiscoveryIssue::Error(DiscoveryError::DuplicateFixture {
                    source_file: visitor.module.source_file(),
                    fixture_name: name.to_string(),
                    first_definition: Rc::clone(first_definition.stmt_function_def()),
                    duplicate_definition: Rc::clone(duplicate_definition.stmt_function_def()),
                }));
        },
    );

    for (index, fixture) in fixtures.into_iter().enumerate() {
        if duplicate_fixture_indices.contains(&index) {
            let name = fixture.name().function_name().to_owned();
            let source_file = visitor.module.source_file();
            let module_path = visitor.module.module_path().clone();
            visitor.module.add_rejected_fixture(RejectedFixture::new(
                name.clone(),
                format!("Fixture `{name}` is defined more than once"),
                Rc::clone(fixture.stmt_function_def()),
                source_file,
                module_path,
            ));
            continue;
        }

        visitor.module.add_fixture(fixture);
    }

    if is_conftest || context.settings().test().try_import_fixtures {
        visitor.find_extra_fixtures();
    }

    visitor.issues
}

fn duplicate_definition_indices<T>(
    definitions: &[T],
    mut name: impl FnMut(&T) -> String,
    mut range: impl FnMut(&T) -> TextRange,
    mut report: impl FnMut(&str, &T, &T),
) -> HashSet<usize> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut duplicates: HashSet<usize> = HashSet::new();

    for (index, definition) in definitions.iter().enumerate() {
        let definition_name = name(definition);

        if let Some(first_index) = seen.get(&definition_name) {
            let first_definition = &definitions[*first_index];
            if range(first_definition) == range(definition) {
                duplicates.insert(index);
                continue;
            }

            report(&definition_name, first_definition, definition);
            duplicates.insert(*first_index);
            duplicates.insert(index);
        } else {
            seen.insert(definition_name, index);
        }
    }

    duplicates
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

/// Finds only top-level functions; nested definitions cannot be imported module fixtures.
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
