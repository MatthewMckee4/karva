//! Resolves explicit fixture imports through indexed project source without executing Python.
//!
//! Unknown exports, wildcard imports and import cycles remain visibility barriers. The public
//! fixture name follows Karva's runtime marker, independently of the local Python import alias.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use crate::semantic::{Binding, PythonSemantics};
use camino::{Utf8Path, Utf8PathBuf};
use karva_collector::{CollectedModule, ModuleType, project_import_paths};
use karva_python_semantic::{DecoratorBindings, KnownBinding};
use ruff_python_ast::visitor::{self, Visitor};
use ruff_python_ast::{Alias, Expr, Stmt, StmtImportFrom};
use ruff_text_size::{Ranged, TextRange};

use super::{FixtureDefinition, FixtureId, FixtureProvider};

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
pub(crate) fn provider_with_imports(
    module: &CollectedModule,
    project_root: &Utf8Path,
    modules: &BTreeMap<Utf8PathBuf, Arc<CollectedModule>>,
    semantics: &PythonSemantics,
    try_import_fixtures: bool,
) -> FixtureProvider {
    let mut provider = semantics.raw_provider(module);
    // `parse_provider` already accounts for dynamic local fixture names. Imports are classified
    // individually below rather than turning every external import into a provider barrier.
    let expose_imports = try_import_fixtures || module.module_type == ModuleType::Configuration;
    let exports = exports(module, semantics);
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
        match resolve_import(
            project_root,
            modules,
            module.path.path(),
            import,
            alias.name.as_str(),
            semantics,
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
            semantics,
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
    Function,

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
    name: &str,
    semantics: &PythonSemantics,
) -> Resolution {
    let Some(source) = project_import_paths(root, path, import)
        .iter()
        .find_map(|path| modules.get(path))
    else {
        return Resolution::Unknown;
    };
    let exports = exports(source, semantics);
    if matches!(exports.names.get(name), Some(Export::NonFixture)) {
        return Resolution::NonFixture;
    }

    if matches!(exports.names.get(name), Some(Export::Unknown))
        || (exports.unknown && !exports.names.contains_key(name))
    {
        return Resolution::Unknown;
    }
    let Some(targets) = semantics.import_targets(path, import.range(), name) else {
        return Resolution::Unknown;
    };
    let [(path, range)] = targets.as_slice() else {
        return Resolution::Unknown;
    };
    let Some(module) = modules.get(path) else {
        return Resolution::Unknown;
    };
    if !semantics.bindings(path).is_some_and(|bindings| bindings.values().any(|binding| matches!(binding, Binding::Definition(declaration) if declaration.contains_range(*range)))) {
        return Resolution::Unknown;
    }
    let provider = semantics.raw_provider(module);
    if let Some(definition) = provider.definitions.into_iter().find(|item| item.name_range == *range) {
        Resolution::Fixture(Box::new(definition))
    } else if module.module_body.iter().any(|statement| matches!(statement, Stmt::FunctionDef(function) if function.name.range() == *range && !function.decorator_list.is_empty())) {
        Resolution::Unknown
    } else {
        Resolution::NonFixture
    }
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
    semantics: &PythonSemantics,
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
    let (names, mut unknown) = exported_names(root, modules, module, semantics, visited);
    let mut fixtures = Vec::new();
    for name in names {
        match resolve_import(root, modules, path, import, &name, semantics) {
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

/// Enumerates names visible to `import *` under Python's `__all__` rules.
///
/// The boolean is true when the set is incomplete because of dynamic exports,
/// conditional wildcard imports, missing source modules, or cycles. The caller
/// may still use the names found, but must retain an unknown barrier.
fn exported_names(
    root: &Utf8Path,
    modules: &BTreeMap<Utf8PathBuf, Arc<CollectedModule>>,
    module: &CollectedModule,
    semantics: &PythonSemantics,
    visited: &mut HashSet<(Utf8PathBuf, String)>,
) -> (HashSet<String>, bool) {
    let key = (module.path.path().clone(), "*".to_owned());
    if !visited.insert(key.clone()) {
        return (HashSet::new(), true);
    }
    let exports = exports(module, semantics);
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
            let (source_names, source_unknown) =
                exported_names(root, modules, source, semantics, visited);
            names.extend(source_names);
            unknown |= source_unknown;
        }
    }
    visited.remove(&key);
    (names, unknown)
}

/// Captures final top-level bindings. Literal values cannot expose callable fixture wrappers.
/// Other assignments and conditional definitions stay unknown without executing Python.
fn exports<'a>(module: &'a CollectedModule, semantics: &PythonSemantics) -> Exports<'a> {
    let Some(bindings) = semantics.bindings(module.path.path()) else {
        return Exports {
            names: BTreeMap::new(),
            wildcards: Vec::new(),
            all: ExportNames::Unknown,
            unknown: true,
        };
    };
    let mut wildcards = Vec::new();
    let all = if bindings.contains_key("__all__") {
        semantics
            .export_names(module.path.path())
            .map_or(ExportNames::Unknown, ExportNames::Explicit)
    } else {
        ExportNames::Default
    };
    let mut unknown = false;
    for statement in &module.module_body {
        if let Stmt::ImportFrom(import) = statement {
            if import.names.iter().any(|alias| alias.name.as_str() == "*") {
                wildcards.push(import);
            }
        }
        let mut finder = ConditionalWildcard::default();
        if !matches!(
            statement,
            Stmt::ImportFrom(_) | Stmt::FunctionDef(_) | Stmt::ClassDef(_)
        ) {
            finder.visit_stmt(statement);
            unknown |= finder.found;
        }
    }
    let names = bindings
        .into_iter()
        .filter(|(_, binding)| !matches!(binding, Binding::Wildcard))
        .map(|(name, binding)| {
            let export = if let Binding::Definition(range) = binding {
                match module
                    .module_body
                    .iter()
                    .find(|statement| statement.range().contains_range(range))
                {
                    Some(Stmt::FunctionDef(_)) => Export::Function,
                    Some(Stmt::ClassDef(_) | Stmt::Import(_)) => Export::NonFixture,
                    Some(Stmt::ImportFrom(import)) => import
                        .names
                        .iter()
                        .find(|alias| alias.asname.as_ref().unwrap_or(&alias.name).as_str() == name)
                        .map_or(Export::Unknown, |alias| {
                            let mut bindings = DecoratorBindings::before(
                                &module.module_body,
                                import.range().start(),
                            );
                            bindings.update(&Stmt::ImportFrom(import.clone()));
                            if name != "__all__"
                                && known_nonfixture_import(import, bindings.get(&name))
                            {
                                Export::NonFixture
                            } else {
                                Export::Import(import, alias)
                            }
                        }),
                    Some(statement)
                        if nonfixture_assignment_names(statement).contains(name.as_str()) =>
                    {
                        Export::NonFixture
                    }
                    _ => Export::Unknown,
                }
            } else {
                Export::Unknown
            };
            (name, export)
        })
        .collect();
    Exports {
        names,
        wildcards,
        all,
        unknown,
    }
}

/// Conditional wildcard imports can introduce otherwise absent fixture names.
#[derive(Default)]
struct ConditionalWildcard {
    found: bool,
}
impl<'a> Visitor<'a> for ConditionalWildcard {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::ImportFrom(import) => {
                self.found |= import.names.iter().any(|alias| alias.name.as_str() == "*");
            }
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
            _ => visitor::walk_stmt(self, statement),
        }
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
