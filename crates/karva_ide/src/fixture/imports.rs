//! Resolves explicit fixture imports through indexed project source without executing Python.
//!
//! Unknown exports, wildcard imports and import cycles remain visibility barriers. The public
//! fixture name follows Karva's runtime marker, independently of the local Python import alias.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use karva_collector::{CollectedModule, ModuleType, project_import_paths};
use karva_python_semantic::{DecoratorBindings, KnownBinding};
use ruff_python_ast::visitor::{self, Visitor};
use ruff_python_ast::{Alias, Expr, ExprContext, Pattern, Stmt, StmtFunctionDef, StmtImportFrom};
use ruff_text_size::{Ranged, TextRange};

use super::{FixtureDefinition, FixtureId, FixtureProvider, parse_provider};

/// An import binding connected to its canonical source provider.
#[derive(Clone, Debug)]
pub(crate) struct FixtureImport {
    /// Local Python binding introduced by the import.
    pub(crate) range: TextRange,

    /// Source binding selected by the import.
    pub(crate) edit_range: TextRange,

    /// Whether the import keeps a distinct local alias.
    pub(crate) has_alias: bool,

    /// Whether the source token is the canonical public fixture name.
    pub(crate) rename_source: bool,

    /// Canonical provider, independent of the import alias.
    pub(crate) fixture: FixtureId,
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
    let exports = exports(module);
    let mut conflicting_names = HashSet::new();
    if exports.unknown || !exports.wildcards.is_empty() {
        let shadowed = provider
            .definitions
            .iter()
            .filter(|definition| {
                matches!(
                    exports.names.get(&definition.defining_name),
                    Some(Export::Unknown)
                )
            })
            .map(|definition| definition.id.clone())
            .collect::<HashSet<_>>();
        conflicting_names.extend(
            provider
                .definitions
                .iter()
                .filter(|definition| shadowed.contains(&definition.id))
                .map(|definition| definition.name.clone()),
        );
        provider
            .definitions
            .retain(|definition| !shadowed.contains(&definition.id));
        provider
            .by_name
            .retain(|_, fixture| !shadowed.contains(fixture));
    }
    provider.unknown |= expose_imports
        && (exports.unknown
            || exports
                .names
                .values()
                .any(|export| matches!(export, Export::Unknown)));
    for export in exports.names.values() {
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
                    edit_range: alias.name.range(),
                    has_alias: alias.asname.is_some(),
                    rename_source: alias.name.as_str() == definition.name,
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
    for import in &exports.wildcards {
        let mut visited = HashSet::new();
        let WildcardResolution { fixtures, unknown } = resolve_wildcard(
            project_root,
            modules,
            module.path.path(),
            import,
            &mut visited,
        );
        provider.unknown |= expose_imports && unknown;
        for WildcardFixture {
            binding,
            definition,
        } in fixtures
        {
            // A later explicit binding wins over a wildcard import. An earlier binding is also
            // treated conservatively because the source-only model does not retain enough order
            // information to prove which side of the wildcard it precedes. The binding is the
            // source module's Python name; `definition.name` may be a different Karva name.
            if let Some(export) = exports.names.get(&binding) {
                provider.unknown |= expose_imports && matches!(export, Export::Unknown);
                continue;
            }
            if conflicting_names.contains(&definition.name) {
                continue;
            }
            let range = import
                .names
                .iter()
                .find(|alias| alias.name.as_str() == "*")
                .map_or_else(|| import.range(), Ranged::range);
            provider.imports.push(FixtureImport {
                range,
                edit_range: range,
                has_alias: false,
                rename_source: false,
                fixture: definition.id.clone(),
            });
            if !expose_imports {
                continue;
            }
            if let Some(existing) = provider.by_name.get(&definition.name) {
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
    }
    provider
}

#[derive(Clone, Copy)]
enum Export<'a> {
    /// A top-level function whose decorator can be checked in the source module.
    Function(&'a StmtFunctionDef),

    /// An explicit `from ... import ...` binding that can be followed.
    Import(&'a StmtImportFrom, &'a Alias),

    /// A binding known to be unrelated to fixture discovery.
    NonFixture,

    /// A binding whose runtime value cannot be established statically.
    Unknown,
}

/// Final top-level export bindings plus wildcard and `__all__` state.
///
/// `unknown` records conditional wildcard imports. It is separate from an
/// [`Export::Unknown`] binding because a conditional import can introduce any
/// name, including one absent from the final binding map.
struct Exports<'a> {
    /// Final explicit bindings by their Python name.
    names: BTreeMap<String, Export<'a>>,

    /// Unconditional wildcard imports in source order.
    wildcards: Vec<&'a StmtImportFrom>,

    /// The public-name policy used by a wildcard import.
    all: ExportNames,

    /// Whether a conditional wildcard makes absent names unknowable.
    unknown: bool,
}

/// Static state of a module's `__all__` binding.
enum ExportNames {
    /// Python's default public-name rule, excluding leading underscores.
    Default,

    /// A literal list or tuple of names, including private names.
    Explicit(Vec<String>),

    /// A dynamic or mutated value that cannot be enumerated safely.
    Unknown,
}

/// Fixtures and uncertainty discovered from one wildcard import.
struct WildcardResolution {
    /// Fixtures that can be resolved to canonical source definitions.
    fixtures: Vec<WildcardFixture>,

    /// Whether any exported name or module state remains unresolved.
    unknown: bool,
}

/// One fixture reached through a wildcard binding.
struct WildcardFixture {
    /// Python name introduced by the wildcard import.
    binding: String,

    /// Canonical Karva fixture definition, independent of `binding`.
    definition: Box<FixtureDefinition>,
}

enum Resolution {
    /// A fixture definition with canonical source identity.
    Fixture(Box<FixtureDefinition>),

    /// A known export that cannot be a fixture.
    NonFixture,

    /// Export state is dynamic, missing, or cyclic.
    Unknown,
}

/// Resolves one explicit source import through the indexed project snapshot.
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
    resolve_export(root, modules, module, alias.name.as_str(), visited)
}

/// Resolves all fixture exports introduced by one unconditional `import *`.
///
/// The returned binding names are Python names from the source module, while
/// definitions retain Karva's public metadata names. Unknown state is kept
/// alongside resolved fixtures so callers can expose useful results without
/// making unsafe diagnostic or navigation claims.
fn resolve_wildcard(
    root: &Utf8Path,
    modules: &BTreeMap<Utf8PathBuf, Arc<CollectedModule>>,
    path: &Utf8Path,
    import: &StmtImportFrom,
    visited: &mut HashSet<(Utf8PathBuf, String)>,
) -> WildcardResolution {
    let Some(module) = project_import_paths(root, path, import)
        .iter()
        .find_map(|path| modules.get(path))
    else {
        return WildcardResolution {
            fixtures: Vec::new(),
            unknown: true,
        };
    };
    let (names, mut unknown) = exported_names(root, modules, module, visited);
    let mut fixtures = Vec::new();
    for name in names {
        match resolve_export(root, modules, module, &name, visited) {
            Resolution::Fixture(definition) => fixtures.push(WildcardFixture {
                binding: name,
                definition,
            }),
            Resolution::Unknown => unknown = true,
            Resolution::NonFixture => {}
        }
    }
    WildcardResolution { fixtures, unknown }
}

/// Resolves one named export while guarding recursive reexports with `visited`.
///
/// A repeated `(module, name)` key means an import cycle, which returns
/// [`Resolution::Unknown`] so editor navigation and diagnostics stay silent.
fn resolve_export(
    root: &Utf8Path,
    modules: &BTreeMap<Utf8PathBuf, Arc<CollectedModule>>,
    module: &CollectedModule,
    name: &str,
    visited: &mut HashSet<(Utf8PathBuf, String)>,
) -> Resolution {
    let key = (module.path.path().clone(), name.to_owned());
    if !visited.insert(key.clone()) {
        return Resolution::Unknown;
    }
    let exports = exports(module);
    let resolution = match exports.names.get(name) {
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
        None if exports.unknown || exports.names.contains_key("__getattr__") => Resolution::Unknown,
        None => resolve_export_from_wildcards(root, modules, module, &exports, name, visited),
    };
    visited.remove(&key);
    resolution
}

/// Resolves a name that is supplied by one of a module's wildcard imports.
///
/// Wildcards are searched in reverse source order because later imports win.
/// An unresolved source is an unknown barrier rather than evidence that the
/// name is non-fixture.
fn resolve_export_from_wildcards(
    root: &Utf8Path,
    modules: &BTreeMap<Utf8PathBuf, Arc<CollectedModule>>,
    module: &CollectedModule,
    exports: &Exports<'_>,
    name: &str,
    visited: &mut HashSet<(Utf8PathBuf, String)>,
) -> Resolution {
    for import in exports.wildcards.iter().rev() {
        let Some(source) = project_import_paths(root, module.path.path(), import)
            .iter()
            .find_map(|path| modules.get(path))
        else {
            return Resolution::Unknown;
        };
        let (names, unknown) = exported_names(root, modules, source, visited);
        if unknown && !names.contains(name) {
            return Resolution::Unknown;
        }
        if names.contains(name) {
            return resolve_export(root, modules, source, name, visited);
        }
    }
    Resolution::NonFixture
}

/// Enumerates names visible to `import *` under Python's `__all__` rules.
///
/// The boolean is true when the set is incomplete because of dynamic exports,
/// conditional wildcard imports, missing source modules, or cycles. The caller
/// may still use the names found, but must retain an unknown barrier.
fn exported_names(
    root: &Utf8Path,
    modules: &BTreeMap<Utf8PathBuf, Arc<CollectedModule>>,
    module: &CollectedModule,
    visited: &mut HashSet<(Utf8PathBuf, String)>,
) -> (HashSet<String>, bool) {
    let key = (module.path.path().clone(), "*".to_owned());
    if !visited.insert(key.clone()) {
        return (HashSet::new(), true);
    }
    let exports = exports(module);
    let mut names = match &exports.all {
        ExportNames::Default => exports
            .names
            .keys()
            .filter(|name| !name.starts_with('_'))
            .cloned()
            .collect(),
        ExportNames::Explicit(names) => names.iter().cloned().collect(),
        ExportNames::Unknown => HashSet::new(),
    };
    let mut unknown = exports.unknown || matches!(&exports.all, ExportNames::Unknown);
    if matches!(&exports.all, ExportNames::Default) {
        for import in &exports.wildcards {
            let Some(source) = project_import_paths(root, module.path.path(), import)
                .iter()
                .find_map(|path| modules.get(path))
            else {
                unknown = true;
                continue;
            };
            let (source_names, source_unknown) = exported_names(root, modules, source, visited);
            names.extend(source_names);
            unknown |= source_unknown;
        }
    }
    visited.remove(&key);
    (names, unknown)
}

/// Captures final top-level bindings. Literal values cannot expose callable fixture wrappers.
/// Other assignments and conditional definitions stay unknown without executing Python.
fn exports(module: &CollectedModule) -> Exports<'_> {
    let mut exports = BTreeMap::new();
    let mut wildcards = Vec::new();
    let mut all = ExportNames::Default;
    let mut unknown = false;
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
                        wildcards.push(import);
                        for export in exports.values_mut() {
                            *export = Export::Unknown;
                        }
                    } else {
                        let name = alias.asname.as_ref().unwrap_or(&alias.name);
                        if name.as_str() == "__all__" {
                            all = ExportNames::Unknown;
                            exports.insert(name.to_string(), Export::Import(import, alias));
                        } else {
                            let mut bindings = DecoratorBindings::before(
                                &module.module_body,
                                statement.range().start(),
                            );
                            bindings.update(statement);
                            let known = bindings.get(name.as_str());
                            if known_nonfixture_import(import, known) {
                                exports.insert(name.to_string(), Export::NonFixture);
                            } else {
                                exports.insert(name.to_string(), Export::Import(import, alias));
                            }
                        }
                    }
                }
            }
            _ => {
                let mut bindings = ChangedBindings::default();
                bindings.visit_stmt(statement);
                let nonfixtures = nonfixture_assignment_names(statement);
                let mut all_usage = ExportAllUsage::default();
                all_usage.visit_stmt(statement);
                if all_usage.used {
                    all = ExportNames::Unknown;
                }
                if let Some(value) = assigned_export_names(statement) {
                    all = value;
                    bindings.names.remove("__all__");
                } else if bindings.names.contains("__all__") {
                    all = ExportNames::Unknown;
                    bindings.names.remove("__all__");
                }
                for name in bindings.names {
                    let export = if nonfixtures.contains(name.as_str()) {
                        Export::NonFixture
                    } else {
                        Export::Unknown
                    };
                    exports.insert(name, export);
                }
                if bindings.wildcard {
                    unknown = true;
                    for export in exports.values_mut() {
                        *export = Export::Unknown;
                    }
                }
            }
        }
    }
    Exports {
        names: exports,
        wildcards,
        all,
        unknown,
    }
}

/// Identifies imports that cannot introduce a fixture value through a wildcard.
///
/// Framework decorator bindings are classified by the shared semantic resolver;
/// only standard-library modules with known non-fixture roles use a module rule.
fn known_nonfixture_import(import: &StmtImportFrom, binding: Option<KnownBinding>) -> bool {
    if import.level == 0
        && import.module.as_ref().is_some_and(|module| {
            matches!(
                module.as_str(),
                "typing" | "collections" | "collections.abc" | "__future__"
            )
        })
    {
        return true;
    }
    matches!(
        binding,
        Some(
            KnownBinding::Karva
                | KnownBinding::Pytest
                | KnownBinding::KarvaTags
                | KnownBinding::PytestMark
                | KnownBinding::Fixture
                | KnownBinding::Parametrize
                | KnownBinding::UseFixtures
        )
    )
}

#[derive(Default)]
struct ExportAllUsage {
    /// Whether `__all__` was read, deleted, or mutated in this statement.
    used: bool,
}

impl<'a> Visitor<'a> for ExportAllUsage {
    fn visit_expr(&mut self, expression: &'a Expr) {
        if let Expr::Name(name) = expression
            && name.id == "__all__"
            && !matches!(name.ctx, ExprContext::Store)
        {
            self.used = true;
        }
        visitor::walk_expr(self, expression);
    }
}

/// Returns a literal `__all__` assignment, or `Unknown` for a dynamic update.
fn assigned_export_names(statement: &Stmt) -> Option<ExportNames> {
    let expression = match statement {
        Stmt::Assign(assign) if assign.targets.iter().any(is_all_target) => {
            Some(assign.value.as_ref())
        }
        Stmt::AnnAssign(assign) if is_all_target(&assign.target) => assign.value.as_deref(),
        Stmt::AugAssign(assign) if is_all_target(&assign.target) => {
            return Some(ExportNames::Unknown);
        }
        _ => None,
    }?;
    Some(literal_export_names(expression).map_or(ExportNames::Unknown, ExportNames::Explicit))
}

/// Returns whether an assignment target is the module-level `__all__` name.
fn is_all_target(expression: &Expr) -> bool {
    matches!(expression, Expr::Name(name) if name.id == "__all__")
}

/// Extracts string names from a literal list or tuple used as `__all__`.
fn literal_export_names(expression: &Expr) -> Option<Vec<String>> {
    let elements = match expression {
        Expr::List(list) => &list.elts,
        Expr::Tuple(tuple) => &tuple.elts,
        _ => return None,
    };
    elements
        .iter()
        .map(|element| match element {
            Expr::StringLiteral(string) => Some(string.value.to_str().to_owned()),
            _ => None,
        })
        .collect()
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
