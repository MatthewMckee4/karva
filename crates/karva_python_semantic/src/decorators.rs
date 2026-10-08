//! Static resolution of Karva and pytest decorator bindings.

use std::collections::HashMap;

use ruff_python_ast::{Expr, Stmt, StmtFunctionDef};
use ruff_text_size::{Ranged, TextSize};

/// A known binding introduced by a Python import.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownBinding {
    /// The `karva` module.
    Karva,

    /// The `pytest` module.
    Pytest,

    /// The `karva.tags` namespace.
    KarvaTags,

    /// The `pytest.mark` namespace.
    PytestMark,

    /// The `fixture` decorator.
    Fixture,

    /// The `parametrize` decorator.
    Parametrize,

    /// The `use_fixtures` or `usefixtures` decorator.
    UseFixtures,

    /// A binding that shadows a known name but has no statically known value.
    Unknown,
}

/// Module-level names that can identify Karva and pytest decorators.
#[derive(Clone, Debug)]
pub struct DecoratorBindings {
    names: HashMap<String, KnownBinding>,
}

impl Default for DecoratorBindings {
    fn default() -> Self {
        let mut names = HashMap::new();
        // Keep the historical shorthand accepted by collection. Imports and
        // assignments can explicitly replace these fallback names.
        names.insert("fixture".to_owned(), KnownBinding::Fixture);
        names.insert("parametrize".to_owned(), KnownBinding::Parametrize);
        Self { names }
    }
}

impl KnownBinding {
    pub(super) fn from_module(module: &str) -> Option<Self> {
        match module {
            "karva" => Some(Self::Karva),
            "pytest" => Some(Self::Pytest),
            "karva.tags" => Some(Self::KarvaTags),
            "pytest.mark" => Some(Self::PytestMark),
            _ => None,
        }
    }

    pub(super) fn from_import(module: &str, name: &str) -> Option<Self> {
        match (module, name) {
            ("karva" | "karva._karva" | "pytest", "fixture") => Some(Self::Fixture),
            ("karva.tags" | "pytest.mark", "parametrize") => Some(Self::Parametrize),
            ("karva.tags", "use_fixtures") | ("pytest.mark", "usefixtures") => {
                Some(Self::UseFixtures)
            }
            ("karva", "tags") => Some(Self::KarvaTags),
            ("pytest", "mark") => Some(Self::PytestMark),
            _ => None,
        }
    }

    pub(super) fn attribute(self, name: &str) -> Option<Self> {
        match (self, name) {
            (Self::Karva, "tags") => Some(Self::KarvaTags),
            (Self::Pytest, "mark") => Some(Self::PytestMark),
            (Self::Karva | Self::Pytest, "fixture") => Some(Self::Fixture),
            (Self::KarvaTags | Self::PytestMark, "parametrize") => Some(Self::Parametrize),
            (Self::KarvaTags, "use_fixtures") | (Self::PytestMark, "usefixtures") => {
                Some(Self::UseFixtures)
            }
            _ => None,
        }
    }
}

impl DecoratorBindings {
    /// Builds a framework name environment from resolved Python bindings.
    #[cfg(feature = "source-analysis")]
    pub(super) fn resolved(names: HashMap<String, KnownBinding>) -> Self {
        Self { names }
    }

    /// Returns permissive bindings used by the legacy syntax-only collector.
    ///
    /// Editor analysis starts with [`Default::default`] so an unimported
    /// module-shaped decorator is not mistaken for a real imported binding.
    pub fn legacy() -> Self {
        let mut bindings = Self::default();
        bindings.set("karva", KnownBinding::Karva);
        bindings.set("pytest", KnownBinding::Pytest);
        bindings
    }

    /// Returns bindings after evaluating the supplied module statements in order.
    pub fn from_statements(statements: &[Stmt]) -> Self {
        let mut bindings = Self::default();
        for statement in statements {
            bindings.update(statement);
        }
        bindings
    }

    /// Returns bindings visible immediately before `offset` in a module.
    pub fn before(statements: &[Stmt], offset: TextSize) -> Self {
        let mut bindings = Self::default();
        for statement in statements {
            if statement.range().start() >= offset {
                break;
            }
            bindings.update(statement);
        }
        bindings
    }

    /// Returns the statically known binding for `name`, including an explicit
    /// [`KnownBinding::Unknown`] shadowing a built-in shorthand.
    pub fn get(&self, name: &str) -> Option<KnownBinding> {
        self.names.get(name).copied()
    }

    /// Applies one module-level statement to the visible-name environment.
    pub fn update(&mut self, statement: &Stmt) {
        match statement {
            Stmt::Import(import) => {
                for alias in &import.names {
                    let name = alias.asname.as_ref().map_or_else(
                        || alias.name.as_str().split('.').next().unwrap_or_default(),
                        ruff_python_ast::Identifier::as_str,
                    );
                    let binding = alias.asname.as_ref().map_or_else(
                        || {
                            KnownBinding::from_module(
                                alias.name.as_str().split('.').next().unwrap_or_default(),
                            )
                        },
                        |_| KnownBinding::from_module(alias.name.as_str()),
                    );
                    self.set(name, binding.unwrap_or(KnownBinding::Unknown));
                }
            }
            Stmt::ImportFrom(import) => {
                let module = import
                    .module
                    .as_ref()
                    .map(ruff_python_ast::Identifier::as_str);
                for alias in &import.names {
                    if alias.name.as_str() == "*" {
                        self.invalidate_known_bindings();
                        continue;
                    }
                    let name = alias
                        .asname
                        .as_ref()
                        .map_or(alias.name.as_str(), ruff_python_ast::Identifier::as_str);
                    let binding = (import.level == 0)
                        .then_some(module)
                        .flatten()
                        .and_then(|module| KnownBinding::from_import(module, alias.name.as_str()))
                        .unwrap_or(KnownBinding::Unknown);
                    self.set(name, binding);
                }
            }
            Stmt::Assign(assign) => {
                for target in &assign.targets {
                    self.bind_target(target);
                }
            }
            Stmt::AnnAssign(assign) if assign.value.is_some() => self.bind_target(&assign.target),
            Stmt::AugAssign(assign) => self.bind_target(&assign.target),
            Stmt::Delete(delete) => {
                for target in &delete.targets {
                    self.bind_target(target);
                }
            }
            Stmt::For(statement) => {
                self.bind_target(&statement.target);
                self.invalidate_statements(&statement.body);
                self.invalidate_statements(&statement.orelse);
            }
            Stmt::With(statement) => {
                for item in &statement.items {
                    if let Some(target) = &item.optional_vars {
                        self.bind_target(target);
                    }
                }
                self.invalidate_statements(&statement.body);
            }
            Stmt::While(statement) => self.invalidate_statements(&statement.body),
            Stmt::If(statement) => {
                self.invalidate_statements(&statement.body);
                for clause in &statement.elif_else_clauses {
                    self.invalidate_statements(&clause.body);
                }
            }
            Stmt::Try(statement) => {
                self.invalidate_statements(&statement.body);
                for handler in &statement.handlers {
                    self.invalidate_handler(handler);
                }
                self.invalidate_statements(&statement.orelse);
                self.invalidate_statements(&statement.finalbody);
            }
            Stmt::Match(statement) => {
                self.invalidate_known_bindings();
                for case in &statement.cases {
                    self.invalidate_statements(&case.body);
                }
            }
            Stmt::FunctionDef(function) => self.set(&function.name, KnownBinding::Unknown),
            Stmt::ClassDef(class) => self.set(&class.name, KnownBinding::Unknown),
            Stmt::Expr(statement) => {
                if let Expr::Named(named) = statement.value.as_ref() {
                    self.bind_target(&named.target);
                }
            }
            _ => {}
        }
    }

    /// Returns whether an expression is a known fixture decorator.
    pub fn is_fixture(&self, expression: &Expr) -> bool {
        self.is_decorator(expression, KnownBinding::Fixture)
    }

    /// Returns whether an expression is a known parametrization decorator.
    pub fn is_parametrize(&self, expression: &Expr) -> bool {
        self.is_decorator(expression, KnownBinding::Parametrize)
    }

    /// Returns whether an expression is a known `usefixtures` decorator.
    pub fn is_use_fixtures(&self, expression: &Expr) -> bool {
        self.is_decorator(expression, KnownBinding::UseFixtures)
    }

    fn is_decorator(&self, expression: &Expr, expected: KnownBinding) -> bool {
        let expression = match expression {
            Expr::Call(call) => call.func.as_ref(),
            expression => expression,
        };
        self.namespace_binding(expression) == Some(expected)
    }

    fn set(&mut self, name: &str, binding: KnownBinding) {
        self.names.insert(name.to_owned(), binding);
    }

    fn namespace_binding(&self, expression: &Expr) -> Option<KnownBinding> {
        match expression {
            Expr::Name(name) => self.get(name.id.as_str()),
            Expr::Attribute(attribute) => {
                let namespace = self.namespace_binding(attribute.value.as_ref())?;
                namespace.attribute(attribute.attr.as_str())
            }
            _ => None,
        }
    }

    fn bind_target(&mut self, target: &Expr) {
        match target {
            Expr::Name(name) => self.set(&name.id, KnownBinding::Unknown),
            Expr::List(list) => {
                for element in &list.elts {
                    self.bind_target(element);
                }
            }
            Expr::Tuple(tuple) => {
                for element in &tuple.elts {
                    self.bind_target(element);
                }
            }
            Expr::Starred(starred) => self.bind_target(starred.value.as_ref()),
            _ => {}
        }
    }

    fn invalidate_statements(&mut self, statements: &[Stmt]) {
        for statement in statements {
            self.invalidate_statement(statement);
        }
    }

    fn invalidate_statement(&mut self, statement: &Stmt) {
        match statement {
            Stmt::Import(import) => {
                for alias in &import.names {
                    let name = alias.asname.as_ref().map_or_else(
                        || alias.name.as_str().split('.').next().unwrap_or_default(),
                        ruff_python_ast::Identifier::as_str,
                    );
                    self.set(name, KnownBinding::Unknown);
                }
            }
            Stmt::ImportFrom(import) => {
                for alias in &import.names {
                    if alias.name.as_str() == "*" {
                        self.invalidate_known_bindings();
                        continue;
                    }
                    let name = alias
                        .asname
                        .as_ref()
                        .map_or(alias.name.as_str(), ruff_python_ast::Identifier::as_str);
                    self.set(name, KnownBinding::Unknown);
                }
            }
            Stmt::Assign(assign) => {
                for target in &assign.targets {
                    self.bind_target(target);
                }
            }
            Stmt::AnnAssign(assign) if assign.value.is_some() => self.bind_target(&assign.target),
            Stmt::AugAssign(assign) => self.bind_target(&assign.target),
            Stmt::Delete(delete) => {
                for target in &delete.targets {
                    self.bind_target(target);
                }
            }
            Stmt::For(statement) => {
                self.bind_target(&statement.target);
                self.invalidate_statements(&statement.body);
                self.invalidate_statements(&statement.orelse);
            }
            Stmt::While(statement) => {
                self.invalidate_statements(&statement.body);
                self.invalidate_statements(&statement.orelse);
            }
            Stmt::If(statement) => {
                self.invalidate_statements(&statement.body);
                for clause in &statement.elif_else_clauses {
                    self.invalidate_statements(&clause.body);
                }
            }
            Stmt::With(statement) => {
                for item in &statement.items {
                    if let Some(target) = &item.optional_vars {
                        self.bind_target(target);
                    }
                }
                self.invalidate_statements(&statement.body);
            }
            Stmt::Try(statement) => {
                self.invalidate_statements(&statement.body);
                for handler in &statement.handlers {
                    self.invalidate_handler(handler);
                }
                self.invalidate_statements(&statement.orelse);
                self.invalidate_statements(&statement.finalbody);
            }
            Stmt::Match(statement) => {
                self.invalidate_known_bindings();
                for case in &statement.cases {
                    self.invalidate_statements(&case.body);
                }
            }
            Stmt::FunctionDef(function) => self.set(&function.name, KnownBinding::Unknown),
            Stmt::ClassDef(class) => self.set(&class.name, KnownBinding::Unknown),
            Stmt::Expr(statement) => {
                if let Expr::Named(named) = statement.value.as_ref() {
                    self.bind_target(&named.target);
                }
            }
            _ => {}
        }
    }

    fn invalidate_handler(&mut self, handler: &ruff_python_ast::ExceptHandler) {
        let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
        if let Some(name) = &handler.name {
            self.set(name, KnownBinding::Unknown);
        }
        self.invalidate_statements(&handler.body);
    }

    fn invalidate_known_bindings(&mut self) {
        for binding in self.names.values_mut() {
            *binding = KnownBinding::Unknown;
        }
    }
}

/// Returns whether a function has a known fixture decorator under `bindings`.
pub fn is_fixture_function_with_bindings(
    function: &StmtFunctionDef,
    bindings: &DecoratorBindings,
) -> bool {
    function
        .decorator_list
        .iter()
        .any(|decorator| bindings.is_fixture(&decorator.expression))
}

#[cfg(test)]
mod tests {
    use ruff_python_ast::{Stmt, StmtFunctionDef};
    use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};

    use super::*;

    fn module(source: &str) -> Vec<Stmt> {
        parse_unchecked(source, ParseOptions::from(Mode::Module))
            .try_into_module()
            .expect("source should parse")
            .into_suite()
            .into_iter()
            .collect()
    }

    fn function(statements: &[Stmt], index: usize) -> &StmtFunctionDef {
        let Stmt::FunctionDef(function) = &statements[index] else {
            panic!("expected function definition")
        };
        function
    }

    #[test]
    fn resolves_fixture_aliases_and_rejects_unrelated_attributes() {
        let statements = module(
            "from karva import fixture as resource\n\n@resource\ndef first(): pass\n\n@other.fixture\ndef second(): pass\n",
        );
        let bindings =
            DecoratorBindings::before(&statements, function(&statements, 1).range.start());
        assert!(is_fixture_function_with_bindings(
            function(&statements, 1),
            &bindings
        ));
        let bindings =
            DecoratorBindings::before(&statements, function(&statements, 2).range.start());
        assert!(!is_fixture_function_with_bindings(
            function(&statements, 2),
            &bindings
        ));
    }

    #[test]
    fn rebinding_shadows_imported_fixture() {
        let statements = module(
            "from karva import fixture as resource\n\n@resource\ndef first(): pass\n\nresource = custom\n\n@resource\ndef second(): pass\n",
        );
        let bindings =
            DecoratorBindings::before(&statements, function(&statements, 1).range.start());
        assert!(is_fixture_function_with_bindings(
            function(&statements, 1),
            &bindings
        ));
        let bindings =
            DecoratorBindings::before(&statements, function(&statements, 3).range.start());
        assert!(!is_fixture_function_with_bindings(
            function(&statements, 3),
            &bindings
        ));
    }

    #[test]
    fn resolves_pytest_namespace_aliases() {
        let statements = module(
            "import pytest as pt\nfrom pytest.mark import parametrize as cases\n\n@pt.fixture\ndef first(): pass\n\n@cases('value', [1])\ndef test_example(value): pass\n",
        );
        let bindings =
            DecoratorBindings::before(&statements, function(&statements, 2).range.start());
        assert!(bindings.is_fixture(&function(&statements, 2).decorator_list[0].expression));
        let test = function(&statements, 3);
        let bindings = DecoratorBindings::before(&statements, test.range.start());
        assert!(bindings.is_parametrize(&test.decorator_list[0].expression));
    }

    #[test]
    fn dotted_import_without_alias_binds_the_root_module() {
        let statements = module(
            "import karva.tags\n\n@karva.tags.parametrize('value', [1])\ndef test_example(value): pass\n",
        );
        let test = function(&statements, 1);
        let bindings = DecoratorBindings::before(&statements, test.range.start());
        assert!(bindings.is_parametrize(&test.decorator_list[0].expression));
    }

    #[test]
    fn control_flow_rebinding_is_unknown_and_relative_imports_are_unrelated() {
        let statements = module(
            "from karva import fixture as resource\n\nif condition:\n    resource = custom\n\n@resource\ndef after_if(): pass\n\ntry:\n    resource = custom\nexcept Exception:\n    pass\n\n@resource\ndef after_try(): pass\n\nfrom .karva import fixture as relative\n@relative\ndef relative_fixture(): pass\n",
        );
        let after_if = function(&statements, 2);
        let bindings = DecoratorBindings::before(&statements, after_if.range.start());
        assert!(!is_fixture_function_with_bindings(after_if, &bindings));
        let after_try = function(&statements, 4);
        let bindings = DecoratorBindings::before(&statements, after_try.range.start());
        assert!(!is_fixture_function_with_bindings(after_try, &bindings));
        let relative_fixture = function(&statements, 6);
        let bindings = DecoratorBindings::before(&statements, relative_fixture.range.start());
        assert!(!is_fixture_function_with_bindings(
            relative_fixture,
            &bindings
        ));
    }

    #[test]
    fn wildcard_and_named_rebindings_shadow_fixture_but_annotations_do_not() {
        let statements = module(
            "from karva import fixture\n\nfixture: object\n@fixture\ndef annotated(): pass\n\n(fixture := custom)\n@fixture\ndef named(): pass\n\nfrom custom import *\n@fixture\ndef wildcard(): pass\n",
        );
        let annotated = function(&statements, 2);
        let bindings = DecoratorBindings::before(&statements, annotated.range.start());
        assert!(is_fixture_function_with_bindings(annotated, &bindings));
        let named = function(&statements, 4);
        let bindings = DecoratorBindings::before(&statements, named.range.start());
        assert!(!is_fixture_function_with_bindings(named, &bindings));
        let wildcard = function(&statements, 6);
        let bindings = DecoratorBindings::before(&statements, wildcard.range.start());
        assert!(!is_fixture_function_with_bindings(wildcard, &bindings));
    }

    #[test]
    fn match_captures_shadow_known_names() {
        let statements = module(
            "from karva import fixture\n\nmatch value:\n    case fixture: pass\n\n@fixture\ndef after_match(): pass\n",
        );
        let after_match = function(&statements, 2);
        let bindings = DecoratorBindings::before(&statements, after_match.range.start());
        assert!(!is_fixture_function_with_bindings(after_match, &bindings));
    }

    #[test]
    fn framework_namespaces_do_not_cross_match() {
        let statements = module(
            "import karva\nimport pytest\n\n@karva.tags.use_fixtures('db')\ndef karva_test(): pass\n\n@pytest.mark.use_fixtures('db')\ndef pytest_test(): pass\n",
        );
        let karva_test = function(&statements, 2);
        let pytest_test = function(&statements, 3);
        let bindings = DecoratorBindings::before(&statements, karva_test.range.start());
        assert!(bindings.is_use_fixtures(&karva_test.decorator_list[0].expression));
        let bindings = DecoratorBindings::before(&statements, pytest_test.range.start());
        assert!(!bindings.is_use_fixtures(&pytest_test.decorator_list[0].expression));
    }
}
