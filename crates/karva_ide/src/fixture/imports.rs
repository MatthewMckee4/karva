//! Resolves explicit fixture imports through indexed project source without executing Python.
//!
//! Unknown exports, wildcard imports and import cycles remain visibility barriers. The public
//! fixture name follows Karva's runtime marker, independently of the local Python import alias.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use karva_collector::{CollectedModule, ModuleType, project_import_paths};
use karva_python_semantic::DecoratorBindings;
use ruff_python_ast::visitor::{self, Visitor};
use ruff_python_ast::{Alias, Expr, ExprContext, Pattern, Stmt, StmtFunctionDef, StmtImportFrom};
use ruff_text_size::{Ranged, TextRange};

use super::{FixtureDefinition, FixtureId, FixtureProvider, parse_provider};

/// An import binding connected to its canonical source provider.
#[derive(Clone, Debug)]
pub(super) struct FixtureImport {
    /// Local Python binding introduced by the import.
    pub(super) range: TextRange,

    /// Canonical provider, independent of the import alias.
    pub(super) fixture: FixtureId,
}

/// Provider parsing with access to the source snapshot used by editor requests.
pub(super) fn provider_with_imports(
    module: &CollectedModule,
    project_root: &Utf8Path,
    modules: &BTreeMap<Utf8PathBuf, Arc<CollectedModule>>,
    try_import_fixtures: bool,
) -> FixtureProvider {
    let mut provider = parse_provider(module, false);
    // `parse_provider` already accounts for dynamic local fixture names. Imports are classified
    // individually below rather than turning every external import into a provider barrier.
    let expose_imports = try_import_fixtures || module.module_type == ModuleType::Configuration;
    provider.unknown = super::has_unknown_fixture_decorator(module)
        || module.fixture_function_defs.iter().any(|function| {
            let parsed = super::parse_fixture(
                function,
                module.path.path(),
                &DecoratorBindings::before(&module.module_body, function.range.start()),
                &module.source_text,
            );
            !parsed.public_name_known
        });
    let (exports, wildcard) = exports(module);
    provider.unknown |= expose_imports
        && (wildcard
            || exports
                .values()
                .any(|export| matches!(export, Export::Unknown)));
    let mut conflicting_names = HashSet::new();
    for export in exports.values() {
        let Export::Import(import, alias) = export else {
            continue;
        };
        if import.level == 0
            && import.module.as_ref().is_some_and(|module| {
                matches!(
                    module.as_str(),
                    "karva" | "pytest" | "typing" | "collections.abc" | "__future__"
                )
            })
        {
            continue;
        }
        let mut visited = HashSet::new();
        match resolve_import(
            project_root,
            modules,
            module.path.path(),
            import,
            alias,
            &mut visited,
        ) {
            Resolution::Fixture(definition) => {
                if conflicting_names.contains(&definition.name) {
                    continue;
                }
                provider.imports.push(FixtureImport {
                    range: alias
                        .asname
                        .as_ref()
                        .map_or_else(|| alias.name.range(), Ranged::range),
                    fixture: definition.id.clone(),
                });
                if !expose_imports {
                    continue;
                }
                if let Some(existing) = provider.by_name.get(&definition.name) {
                    // Reexporting the same provider is harmless; distinct providers with the same
                    // runtime public name cannot be chosen confidently by source-only analysis.
                    if existing != &definition.id {
                        provider.unknown = true;
                        conflicting_names.insert(definition.name.clone());
                        provider
                            .definitions
                            .retain(|item| item.name != definition.name);
                        provider.by_name.remove(&definition.name);
                    }
                } else {
                    provider
                        .by_name
                        .insert(definition.name.clone(), definition.id.clone());
                    provider.definitions.push(*definition);
                }
            }
            Resolution::Unknown => provider.unknown |= expose_imports,
            Resolution::NonFixture => {}
        }
    }
    provider
}

#[derive(Clone, Copy)]
enum Export<'a> {
    Function(&'a StmtFunctionDef),
    Import(&'a StmtImportFrom, &'a Alias),
    NonFixture,
    Unknown,
}

enum Resolution {
    Fixture(Box<FixtureDefinition>),
    NonFixture,
    Unknown,
}

fn resolve_import(
    root: &Utf8Path,
    modules: &BTreeMap<Utf8PathBuf, Arc<CollectedModule>>,
    path: &Utf8Path,
    import: &StmtImportFrom,
    alias: &Alias,
    visited: &mut HashSet<(Utf8PathBuf, String)>,
) -> Resolution {
    let Some(module) = project_import_paths(root, path, import)
        .iter()
        .find_map(|path| modules.get(path))
    else {
        return Resolution::Unknown;
    };
    let key = (module.path.path().clone(), alias.name.to_string());
    if !visited.insert(key.clone()) {
        return Resolution::Unknown;
    }
    let (exports, wildcard) = exports(module);
    let resolution = match exports.get(alias.name.as_str()) {
        Some(Export::Function(function)) => {
            let provider = parse_provider(module, false);
            if let Some(definition) = provider
                .definitions
                .into_iter()
                .find(|item| item.defining_name == function.name.as_str())
            {
                Resolution::Fixture(Box::new(definition))
            } else if function.decorator_list.is_empty() {
                Resolution::NonFixture
            } else {
                Resolution::Unknown
            }
        }
        Some(Export::Import(import, alias)) => {
            resolve_import(root, modules, module.path.path(), import, alias, visited)
        }
        Some(Export::NonFixture) => Resolution::NonFixture,
        Some(Export::Unknown) => Resolution::Unknown,
        None if wildcard || exports.contains_key("__getattr__") => Resolution::Unknown,
        None => Resolution::NonFixture,
    };
    visited.remove(&key);
    resolution
}

/// Captures final top-level bindings. Literal values cannot expose callable fixture wrappers.
/// Other assignments and conditional definitions stay unknown without executing Python.
fn exports(module: &CollectedModule) -> (BTreeMap<String, Export<'_>>, bool) {
    let mut exports = BTreeMap::new();
    let mut wildcard = false;
    for statement in &module.module_body {
        match statement {
            Stmt::FunctionDef(function) => {
                exports.insert(function.name.to_string(), Export::Function(function));
            }
            Stmt::ClassDef(class) => {
                exports.insert(class.name.to_string(), Export::NonFixture);
            }
            Stmt::Import(import) => {
                for alias in &import.names {
                    let name = alias.asname.as_ref().map_or_else(
                        || {
                            alias
                                .name
                                .as_str()
                                .split('.')
                                .next()
                                .unwrap_or(alias.name.as_str())
                        },
                        |name| name.as_str(),
                    );
                    exports.insert(name.to_owned(), Export::NonFixture);
                }
            }
            Stmt::ImportFrom(import) => {
                for alias in &import.names {
                    if alias.name.as_str() == "*" {
                        wildcard = true;
                        for export in exports.values_mut() {
                            *export = Export::Unknown;
                        }
                    } else {
                        let name = alias.asname.as_ref().unwrap_or(&alias.name);
                        exports.insert(name.to_string(), Export::Import(import, alias));
                    }
                }
            }
            _ => {
                let mut bindings = ChangedBindings::default();
                bindings.visit_stmt(statement);
                let nonfixtures = nonfixture_assignment_names(statement);
                for name in bindings.names {
                    let export = if nonfixtures.contains(name.as_str()) {
                        Export::NonFixture
                    } else {
                        Export::Unknown
                    };
                    exports.insert(name, export);
                }
                if bindings.wildcard {
                    wildcard = true;
                    for export in exports.values_mut() {
                        *export = Export::Unknown;
                    }
                }
            }
        }
    }
    (exports, wildcard)
}

/// Restricts literal classification to complete named assignment targets. Unpacking can expose
/// callable elements, and named expressions inside a value may independently bind fixtures.
fn nonfixture_assignment_names(statement: &Stmt) -> HashSet<&str> {
    match statement {
        Stmt::Assign(assign) if is_nonfixture_value(&assign.value) => assign
            .targets
            .iter()
            .filter_map(|target| match target {
                Expr::Name(name) => Some(name.id.as_str()),
                _ => None,
            })
            .collect(),
        Stmt::AnnAssign(assign)
            if assign
                .value
                .as_ref()
                .is_some_and(|value| is_nonfixture_value(value)) =>
        {
            if let Expr::Name(name) = assign.target.as_ref() {
                HashSet::from([name.id.as_str()])
            } else {
                HashSet::new()
            }
        }
        _ => HashSet::new(),
    }
}

/// These expressions produce built-in values that cannot carry Karva's callable fixture marker.
/// Container elements may be fixtures, but runtime discovery scans module values, not elements.
fn is_nonfixture_value(expression: &Expr) -> bool {
    match expression {
        Expr::NumberLiteral(_)
        | Expr::BooleanLiteral(_)
        | Expr::NoneLiteral(_)
        | Expr::EllipsisLiteral(_)
        | Expr::StringLiteral(_)
        | Expr::BytesLiteral(_)
        | Expr::FString(_)
        | Expr::List(_)
        | Expr::Tuple(_)
        | Expr::Set(_)
        | Expr::Dict(_) => true,
        Expr::UnaryOp(unary) => matches!(
            unary.operand.as_ref(),
            Expr::NumberLiteral(_) | Expr::BooleanLiteral(_)
        ),
        _ => false,
    }
}

#[derive(Default)]
struct ChangedBindings {
    names: HashSet<String>,
    wildcard: bool,
}

impl<'a> Visitor<'a> for ChangedBindings {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::AnnAssign(assign) if assign.value.is_none() => {}
            Stmt::FunctionDef(function) => {
                self.names.insert(function.name.to_string());
            }
            Stmt::ClassDef(class) => {
                self.names.insert(class.name.to_string());
            }
            Stmt::Import(import) => {
                for alias in &import.names {
                    self.names
                        .insert(alias.asname.as_ref().unwrap_or(&alias.name).to_string());
                }
            }
            Stmt::ImportFrom(import) => {
                for alias in &import.names {
                    if alias.name.as_str() == "*" {
                        self.wildcard = true;
                    } else {
                        self.names
                            .insert(alias.asname.as_ref().unwrap_or(&alias.name).to_string());
                    }
                }
            }
            _ => visitor::walk_stmt(self, statement),
        }
    }

    fn visit_pattern(&mut self, pattern: &'a Pattern) {
        let name = match pattern {
            Pattern::MatchAs(pattern) => pattern.name.as_ref(),
            Pattern::MatchStar(pattern) => pattern.name.as_ref(),
            Pattern::MatchMapping(pattern) => pattern.rest.as_ref(),
            _ => None,
        };
        if let Some(name) = name {
            self.names.insert(name.to_string());
        }
        visitor::walk_pattern(self, pattern);
    }

    fn visit_except_handler(&mut self, handler: &'a ruff_python_ast::ExceptHandler) {
        let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
        if let Some(name) = &handler.name {
            self.names.insert(name.to_string());
        }
        visitor::walk_body(self, &handler.body);
    }

    fn visit_expr(&mut self, expression: &'a Expr) {
        if let Expr::Name(name) = expression {
            if matches!(name.ctx, ExprContext::Store | ExprContext::Del) {
                self.names.insert(name.id.to_string());
            }
        } else if !matches!(expression, Expr::Lambda(_)) {
            visitor::walk_expr(self, expression);
        }
    }
}
