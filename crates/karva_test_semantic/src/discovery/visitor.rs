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
use ruff_source_file::{LineIndex, SourceFileBuilder};
use ruff_text_size::TextRange;

use crate::Context;
use crate::discovery::{DiscoveredModule, DiscoveredTestFunction, DiscoveryError, DiscoveryIssue};
use crate::extensions::fixtures::python::FixtureFunctionDefinition;
use crate::extensions::fixtures::{DiscoveredFixture, RejectedFixture};
use crate::extensions::tags::Tags;
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

    /// Complete module statements used to locate runtime-discovered module marks.
    module_body: Box<[Stmt]>,

    /// Lazily-loaded Python module, imported only when needed to avoid side effects.
    py_module: Option<Bound<'py, PyModule>>,

    /// Python interpreter handle for this visitor.
    py: Python<'py>,

    /// Flag to prevent multiple import attempts for the same module.
    tried_to_import_module: bool,

    /// Module-level native tags parsed once and shared by every test.
    module_tags: Tags,

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
            module_tags: Tags::default(),
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
            Ok(py_module) => match py_module.dict().get_item("karva_tag") {
                Ok(Some(module_tags)) => match Tags::from_karva_module_value(self.py, &module_tags)
                {
                    Ok(module_tags) => {
                        self.module_tags = module_tags;
                        self.py_module = Some(py_module);
                    }
                    Err(error) => self
                        .issues
                        .push(DiscoveryIssue::Error(DiscoveryError::Import {
                            module_name: self.module.name().to_string(),
                            reason: error.value(self.py).to_string(),
                        })),
                },
                Ok(None) => self.py_module = Some(py_module),
                Err(error) => self
                    .issues
                    .push(DiscoveryIssue::Error(DiscoveryError::Import {
                        module_name: self.module.name().to_string(),
                        reason: error.value(self.py).to_string(),
                    })),
            },
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
        is_conftest: bool,
    ) -> Option<(DiscoveredFixture, Rc<StmtFunctionDef>)> {
        self.try_import_module();

        let py_module = self.py_module.as_ref()?;

        let name = stmt_function_def.name.as_str();
        if let Ok(Some(value)) = py_module.dict().get_item(name)
            && let Ok(function) = DiscoveredFixture::source_function(&value)
            && self.is_other_fixture_definition(&function, &stmt_function_def)
        {
            // Execute the replacement, but retain its declaration for duplicate diagnostics.
            let fixture = self.resolve_runtime_fixture(name, &value, is_conftest, false)?;
            return Some((fixture, Rc::new(stmt_function_def)));
        }

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
            Ok(fixture_def) => Some((fixture_def, stmt_function_def)),
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

    /// Detects a replacement callable while leaving duplicate fixture declarations to validation.
    fn is_other_fixture_definition(
        &self,
        function: &Bound<'_, PyAny>,
        definition: &StmtFunctionDef,
    ) -> bool {
        let Ok(qualified_name) = function
            .getattr("__qualname__")
            .and_then(|value| value.extract::<String>())
        else {
            return false;
        };
        if qualified_name.contains('.') {
            return false;
        }
        if qualified_name != definition.name.as_str()
            || function
                .getattr("__module__")
                .and_then(|value| value.extract::<String>())
                .is_ok_and(|module| module != self.module.name())
        {
            return true;
        }

        let Some(line) = function_first_line(function) else {
            return false;
        };
        let source_file = self.module.source_file();
        let source = source_file.to_source_code();
        if source.line_index(definition.range.start()).get() == line {
            return false;
        }

        matching_module_functions(&self.module_body, &qualified_name)
            .into_iter()
            .any(|candidate| source.line_index(candidate.range.start()).get() == line)
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
                &self.module_tags,
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
                &self.module_tags,
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

    fn find_extra_fixtures(&mut self, is_conftest: bool) {
        let Some(py_module) = self.py_module.clone() else {
            return;
        };

        // Inspecting a fixture can execute Python, including scope callbacks.
        let bindings: Vec<_> = py_module.dict().iter().collect();
        for (name_obj, value) in bindings {
            let Ok(name) = name_obj.extract::<String>() else {
                continue;
            };
            if value.is_callable()
                && is_fixture_value(&value)
                && let Some(fixture) = self.resolve_extra_fixture(&name, &value, is_conftest)
            {
                self.module.add_fixture(fixture);
            }
        }
    }

    fn resolve_extra_fixture(
        &mut self,
        name: &str,
        value: &Bound<'_, PyAny>,
        is_conftest: bool,
    ) -> Option<DiscoveredFixture> {
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

        self.resolve_runtime_fixture(name, value, is_conftest, true)
    }

    /// Resolves a runtime binding against the callable's source definition.
    /// Additional bindings share the provider's public namespace, while declared fixtures
    /// enter duplicate validation with their original definition sites.
    fn resolve_runtime_fixture(
        &mut self,
        name: &str,
        value: &Bound<'_, PyAny>,
        is_conftest: bool,
        is_extra_binding: bool,
    ) -> Option<DiscoveredFixture> {
        let function = DiscoveredFixture::source_function(value).ok()?;
        let module_name = function
            .getattr("__module__")
            .ok()?
            .extract::<String>()
            .ok()?;
        let imported_module = self.py.import(&module_name).ok()?;
        let file_name = imported_module
            .getattr("__file__")
            .ok()?
            .extract::<String>()
            .ok()?;
        let utf8_file_name = Utf8Path::from_path(Path::new(&file_name))?;
        let provider_path = if is_conftest {
            ModulePath::new(utf8_file_name, self.context.cwd())?
        } else {
            // Different test modules can bind the same name to different callables.
            self.module.module_path().clone()
        };
        let func_name = function
            .getattr("__name__")
            .ok()?
            .extract::<String>()
            .ok()?;

        let public_name = DiscoveredFixture::exposure_name_from_value(
            value,
            &func_name,
            (!is_conftest).then_some(name),
        );
        if is_extra_binding
            && let Ok(public_name) = &public_name
            && (self
                .module
                .fixtures()
                .iter()
                .any(|fixture| fixture.name().function_name() == public_name)
                || self.module.rejected_fixture(public_name).is_some())
        {
            return None;
        }

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

        let stmt_function_def = find_function_statement(
            &func_name,
            function_first_line(&function),
            &source_text,
            self.context.python_version(),
        )?;

        let is_generator_function = is_generator(&stmt_function_def);
        let source_file = SourceFileBuilder::new(utf8_file_name.as_str(), source_text).finish();
        let rejected_name = if is_conftest {
            name
        } else {
            public_name.as_deref().unwrap_or(name)
        }
        .to_owned();

        match public_name.and_then(|name| {
            DiscoveredFixture::try_from_value(
                self.py,
                stmt_function_def.clone(),
                value,
                &provider_path,
                source_file.clone(),
                is_generator_function,
                name,
            )
        }) {
            Ok(fixture_def) => Some(fixture_def),
            Err(error) => {
                self.module.add_rejected_fixture(RejectedFixture::new(
                    rejected_name,
                    error.value(self.py).to_string(),
                    Rc::clone(&stmt_function_def),
                    source_file.clone(),
                    provider_path,
                ));
                self.issues
                    .push(DiscoveryIssue::Error(DiscoveryError::InvalidFixture {
                        source_file,
                        definition: stmt_function_def,
                        reason: error.value(self.py).to_string(),
                    }));
                None
            }
        }
    }
}

/// Binds collected AST definitions to imported Python callables.
///
/// Duplicate or invalid definitions emit diagnostics and are excluded. The runtime namespace
/// supplies imported and assigned fixtures once the module has been loaded. The module body
/// provides source ranges for marks inherited by each test at runtime.
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
        if let Some(fixture) = visitor.process_fixture_function(fixture_function_def, is_conftest) {
            fixtures.push(fixture);
        }
    }

    let duplicate_fixture_indices = duplicate_definition_indices(
        &fixtures,
        |(fixture, _)| fixture.name().function_name().to_string(),
        |(_, declaration)| declaration.range,
        |name, (_, first_declaration), (_, duplicate_declaration)| {
            visitor
                .issues
                .push(DiscoveryIssue::Error(DiscoveryError::DuplicateFixture {
                    source_file: visitor.module.source_file(),
                    fixture_name: name.to_string(),
                    first_definition: Rc::clone(first_declaration),
                    duplicate_definition: Rc::clone(duplicate_declaration),
                }));
        },
    );

    for (index, (fixture, declaration)) in fixtures.into_iter().enumerate() {
        if duplicate_fixture_indices.contains(&index) {
            let name = fixture.name().function_name().to_owned();
            let source_file = visitor.module.source_file();
            let module_path = visitor.module.module_path().clone();
            visitor.module.add_rejected_fixture(RejectedFixture::new(
                name.clone(),
                format!("Fixture `{name}` is defined more than once"),
                declaration,
                source_file,
                module_path,
            ));
            continue;
        }

        visitor.module.add_fixture(fixture);
    }

    if is_conftest || context.settings().test().try_import_fixtures {
        visitor.try_import_module();
    }
    visitor.find_extra_fixtures(is_conftest);

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

/// `CPython` uses the first decorator's line, or the function's first line when undecorated.
fn function_first_line(function: &Bound<'_, PyAny>) -> Option<usize> {
    function
        .getattr("__code__")
        .and_then(|code| code.getattr("co_firstlineno"))
        .and_then(|line| line.extract())
        .ok()
}

/// Returns `true` if `value` is a fixture — either a pytest-decorated function
/// (detected via `_fixture_function_marker` / `_pytestfixturefunction`) or a
/// Karva `FixtureFunctionDefinition` object.
fn is_fixture_value(value: &Bound<'_, PyAny>) -> bool {
    value.getattr("_fixture_function_marker").is_ok()
        || value.getattr("_pytestfixturefunction").is_ok()
        || value.cast::<FixtureFunctionDefinition>().is_ok()
}

/// Matches repeated definitions by the callable's source line.
fn find_function_statement(
    name: &str,
    first_line: Option<usize>,
    source_text: &str,
    python_version: PythonVersion,
) -> Option<Rc<StmtFunctionDef>> {
    let mut parse_options = ParseOptions::from(Mode::Module);

    parse_options = parse_options.with_target_version(python_version);

    let parsed = parse_unchecked(source_text, parse_options).try_into_module()?;

    let module = parsed.into_syntax();
    let definitions = matching_module_functions(&module.body, name);
    let definition = if let [definition] = definitions.as_slice() {
        *definition
    } else {
        let first_line = first_line?;
        let lines = LineIndex::from_source_text(source_text);
        definitions
            .into_iter()
            .find(|definition| lines.line_index(definition.range.start()).get() == first_line)?
    };
    Some(Rc::new(definition.clone()))
}

/// Finds definitions in the module's namespace, including control-flow blocks.
fn matching_module_functions<'ast>(body: &'ast [Stmt], name: &str) -> Vec<&'ast StmtFunctionDef> {
    struct MatchingFunctions<'ast, 'name> {
        name: &'name str,
        definitions: Vec<&'ast StmtFunctionDef>,
    }

    impl<'ast> SourceOrderVisitor<'ast> for MatchingFunctions<'ast, '_> {
        fn visit_stmt(&mut self, stmt: &'ast Stmt) {
            match stmt {
                Stmt::FunctionDef(definition) => {
                    if definition.name.as_str() == self.name {
                        self.definitions.push(definition);
                    }
                }
                Stmt::ClassDef(_) => {}
                _ => source_order::walk_stmt(self, stmt),
            }
        }

        fn visit_expr(&mut self, _expr: &Expr) {}
    }

    let mut finder = MatchingFunctions {
        name,
        definitions: Vec::new(),
    };
    source_order::walk_body(&mut finder, body);
    finder.definitions
}
