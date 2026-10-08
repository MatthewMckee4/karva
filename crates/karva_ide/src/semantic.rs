//! Python binding and type queries backed by ty's incremental semantic database.
//!
//! The database sees exactly the immutable source snapshot supplied by Karva, including unsaved
//! documents. Its filesystem cannot execute Python or read unrelated project files.

#![expect(
    clippy::redundant_pub_crate,
    reason = "Sibling IDE modules consume the private ty adapter"
)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use camino::{Utf8Path, Utf8PathBuf};
use karva_collector::CollectedModule;
use ruff_db::files::{File, system_path_to_file};
use ruff_db::parsed::{ParsedModuleRef, parsed_module};
use ruff_db::system::{InMemorySystem, MemoryFileSystem, SystemPath, SystemPathBuf};
use ruff_python_ast::visitor::{self, Visitor};
use ruff_python_ast::{Expr, ExprName, PythonVersion, Stmt, StmtFunctionDef};
use ruff_ranged_value::RangedValue;
use ruff_text_size::{Ranged, TextRange};
use ty_project::metadata::Options;
use ty_project::metadata::options::EnvironmentOptions;
use ty_project::{ProjectDatabase, ProjectMetadata};
use ty_python_core::scope::{FileScopeId, ScopeKind};
use ty_python_core::{global_scope, place_table, semantic_index, use_def_map};
use ty_python_semantic::types::ide_support::{
    ImportAliasResolution, definitions_for_imported_symbol,
};
use ty_python_semantic::types::{Type, TypeDefinition};
use ty_python_semantic::{Db, HasType, SemanticModel};

/// Immutable source view with snapshot-local results and shared incremental ty queries.
#[derive(Debug)]
pub(super) struct PythonSemantics {
    cache: Arc<PythonSemanticCache>,
    root: Utf8PathBuf,
    modules: Arc<BTreeMap<Utf8PathBuf, Arc<CollectedModule>>>,
    python_version: PythonVersion,
    raw_providers: BTreeMap<Utf8PathBuf, OnceLock<crate::fixture::FixtureProvider>>,
    providers: BTreeMap<Utf8PathBuf, OnceLock<crate::fixture::FixtureProvider>>,
    value_types: HashMap<crate::FixtureId, OnceLock<Option<String>>>,
}

/// One final Python binding, as determined by ty's control-flow analysis.
pub(super) enum Binding {
    /// Exactly one reaching declaration in the module.
    Definition(TextRange),
    /// Multiple, missing or conditional declarations cannot be selected safely.
    Unknown,
    /// A name contributed solely by a wildcard import; Karva checks its export policy separately.
    Wildcard,
}

/// Incremental ty queries retained across immutable editor source generations.
///
/// Database access is serialized so a source update never cancels another request's query.
/// Each query activates its own immutable source view, including requests for older snapshots.
#[derive(Debug, Default)]
pub struct PythonSemanticCache {
    project: Mutex<Option<SemanticProject>>,

    /// Framework metadata depends on collected syntax, independently of inferred Python types.
    providers:
        Mutex<BTreeMap<Utf8PathBuf, (Arc<CollectedModule>, crate::fixture::FixtureProvider)>>,
}

#[derive(Debug)]
struct SemanticProject {
    db: ProjectDatabase,
    filesystem: MemoryFileSystem,
    root: Utf8PathBuf,
    python_version: PythonVersion,
    modules: Arc<BTreeMap<Utf8PathBuf, Arc<CollectedModule>>>,
}

/// A query borrows ty's own lexical tables rather than copying Python scope semantics.
struct NameQuery<'db, 'ast> {
    db: &'db dyn Db,
    index: &'db ty_python_core::SemanticIndex<'db>,
    ast: &'ast ParsedModuleRef,
    root: FileScopeId,
    range: TextRange,
    names: Vec<&'ast ExprName>,
}

#[derive(Default)]
struct Names<'a> {
    names: Vec<&'a ExprName>,
    range: TextRange,
}
impl<'a> Visitor<'a> for Names<'a> {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        if statement.range().intersect(self.range).is_some() {
            visitor::walk_stmt(self, statement);
        }
    }
    fn visit_expr(&mut self, expression: &'a Expr) {
        if expression.range().intersect(self.range).is_none() {
            return;
        }
        if let Expr::Name(name) = expression {
            self.names.push(name);
        } else {
            visitor::walk_expr(self, expression);
        }
    }
}

impl NameQuery<'_, '_> {
    fn binding(&self, name: &ExprName) -> Option<FileScopeId> {
        let scope = self
            .index
            .try_expression_scope_id(&ruff_python_ast::ExprRef::Name(name))?;
        self.index
            .visible_ancestor_scopes(scope)
            .find_map(|(scope, _)| {
                let symbol = self
                    .index
                    .place_table(scope)
                    .symbol_by_name(name.id.as_str())?;
                if symbol.is_global() {
                    Some(FileScopeId::global())
                } else {
                    (symbol.is_bound() && !symbol.is_nonlocal()).then_some(scope)
                }
            })
    }

    fn scope_range(&self, scope: FileScopeId) -> Option<TextRange> {
        self.index
            .scope(scope)
            .node()
            .node_index()
            .map(|index| self.ast.get_by_index(index).range())
    }

    fn unsupported(&self, names: &HashSet<String>) -> bool {
        self.index.scope_ids().any(|id| {
            let scope = id.file_scope_id(self.db);
            self.scope_range(scope)
                .is_some_and(|range| self.range.contains_range(range))
                && self.index.place_table(scope).symbols().any(|symbol| {
                    names.contains(symbol.name().as_str())
                        && (symbol.is_global()
                            || symbol.is_nonlocal()
                            || id.node(self.db).scope_kind() == ScopeKind::Class
                                && symbol.is_bound())
                })
        })
    }
}

impl PythonSemantics {
    /// Creates isolated semantics without reading project files or executing Python.
    #[cfg(test)]
    fn new(
        root: &Utf8Path,
        modules: &BTreeMap<Utf8PathBuf, Arc<CollectedModule>>,
        python_version: PythonVersion,
    ) -> Self {
        Self::with_cache(root, modules, python_version, Arc::default())
    }

    pub(super) fn with_cache(
        root: &Utf8Path,
        modules: &BTreeMap<Utf8PathBuf, Arc<CollectedModule>>,
        python_version: PythonVersion,
        cache: Arc<PythonSemanticCache>,
    ) -> Self {
        Self {
            cache,
            root: root.to_owned(),
            python_version,
            modules: Arc::new(modules.clone()),
            raw_providers: modules
                .keys()
                .map(|path| (path.clone(), OnceLock::new()))
                .collect(),
            providers: modules
                .keys()
                .map(|path| (path.clone(), OnceLock::new()))
                .collect(),
            value_types: modules
                .values()
                .flat_map(|module| {
                    module.fixture_function_defs.iter().map(|function| {
                        (
                            crate::FixtureId {
                                path: module.path.path().clone(),
                                range: function.name.range(),
                            },
                            OnceLock::new(),
                        )
                    })
                })
                .collect(),
        }
    }

    fn create_project(&self) -> std::io::Result<SemanticProject> {
        let root = &self.root;
        let modules = &self.modules;
        let python_version = self.python_version;
        let filesystem = MemoryFileSystem::with_current_directory(SystemPath::new(root.as_str()));
        for module in modules.values() {
            filesystem.write_file_all(
                SystemPath::new(module.path.path().as_str()),
                &module.source_text,
            )?;
        }
        let mut metadata = ProjectMetadata::new("Karva", SystemPathBuf::from(root.as_str()));
        let version = python_version.try_into().map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Unsupported ty Python version: {error}"),
            )
        })?;
        metadata.set_override_options(Options {
            environment: Some(EnvironmentOptions {
                python_version: Some(RangedValue::cli(version)),
                ..EnvironmentOptions::default()
            }),
            ..Options::default()
        });
        Ok(SemanticProject {
            db: ProjectDatabase::use_defaults(
                metadata,
                InMemorySystem::from_memory_fs(filesystem.clone()),
            ),
            filesystem,
            root: self.root.clone(),
            python_version,
            modules: Arc::clone(&self.modules),
        })
    }

    fn with_database<T>(&self, query: impl FnOnce(&ProjectDatabase) -> Option<T>) -> Option<T> {
        let mut cached = self.cache.project.lock().ok()?;
        if cached.as_ref().is_none_or(|project| {
            project.root != self.root || project.python_version != self.python_version
        }) {
            *cached = Some(self.create_project().ok()?);
        }
        let project = cached.as_mut()?;
        if !Arc::ptr_eq(&project.modules, &self.modules) {
            for path in project
                .modules
                .keys()
                .filter(|path| !self.modules.contains_key(*path))
            {
                project
                    .filesystem
                    .remove_file(SystemPath::new(path.as_str()))
                    .ok()?;
                File::sync_path(&mut project.db, SystemPath::new(path.as_str()));
            }
            for (path, module) in self.modules.iter() {
                if project.modules.get(path).is_some_and(|previous| {
                    Arc::ptr_eq(previous, module) || previous.source_text == module.source_text
                }) {
                    continue;
                }
                project
                    .filesystem
                    .write_file_all(SystemPath::new(path.as_str()), &module.source_text)
                    .ok()?;
                File::sync_path(&mut project.db, SystemPath::new(path.as_str()));
            }
            project.modules = Arc::clone(&self.modules);
        }
        query(&project.db)
    }

    /// Shares framework metadata and inferred value types between importing modules.
    pub(super) fn raw_provider(&self, module: &CollectedModule) -> crate::fixture::FixtureProvider {
        let build = || {
            let Some(source) = self.modules.get(module.path.path()) else {
                return crate::fixture::parse_provider(module);
            };
            let mut providers = self
                .cache
                .providers
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((previous, provider)) = providers.get(module.path.path())
                && Arc::ptr_eq(previous, source)
            {
                return provider.clone();
            }
            let provider = crate::fixture::parse_provider(module);
            providers.insert(
                module.path.path().to_owned(),
                (Arc::clone(source), provider.clone()),
            );
            provider
        };
        self.raw_providers
            .get(module.path.path())
            .map_or_else(build, |cached| cached.get_or_init(build).clone())
    }

    /// Infers only a requested provider value, avoiding type-checking during fixture diagnostics.
    pub(super) fn value_type(&self, id: &crate::FixtureId) -> Option<String> {
        self.value_types
            .get(id)?
            .get_or_init(|| {
                let module = self.modules.get(&id.path)?;
                let function = module
                    .fixture_function_defs
                    .iter()
                    .find(|function| function.name.range() == id.range)?;
                self.fixture_value_type(&id.path, function)
            })
            .clone()
    }

    /// Parses each provider once per immutable snapshot, independently of its consumers.
    pub(super) fn provider(
        &self,
        module: &CollectedModule,
        modules: &BTreeMap<Utf8PathBuf, Arc<CollectedModule>>,
        try_import_fixtures: bool,
    ) -> crate::fixture::FixtureProvider {
        let build = || {
            crate::fixture::imports::provider_with_imports(
                module,
                self.root.as_path(),
                modules,
                self,
                try_import_fixtures,
            )
        };
        self.providers
            .get(module.path.path())
            .map_or_else(build, |cached| cached.get_or_init(build).clone())
    }

    fn with_names<T>(
        &self,
        path: &Utf8Path,
        root: TextRange,
        body: TextRange,
        query: impl FnOnce(&NameQuery<'_, '_>) -> Option<T>,
    ) -> Option<T> {
        self.with_database(|db| {
            let file = system_path_to_file(db, SystemPath::new(path.as_str())).ok()?;
            let program_file = db.program_file(file);
            let ast = parsed_module(db, program_file.python_file(db)).load(db);
            let index = semantic_index(db, program_file);
            let root_scope = if root.is_empty() {
                FileScopeId::global()
            } else {
                index.scope_ids().find_map(|id| {
                    let node = id.node(db);
                    (matches!(node.scope_kind(), ScopeKind::Function | ScopeKind::Class)
                        && node
                            .node_index()
                            .is_some_and(|index| ast.get_by_index(index).range() == root))
                    .then_some(id.file_scope_id(db))
                })?
            };
            let mut visitor = Names {
                names: Vec::new(),
                range: body,
            };
            visitor.visit_body(&ast.syntax().body);
            query(&NameQuery {
                db,
                index,
                ast: &ast,
                root: root_scope,
                range: if root.is_empty() { body } else { root },
                names: visitor.names,
            })
        })
    }

    /// Connects names to injected parameters or imported bindings through ty's lexical tables.
    pub(super) fn references(
        &self,
        path: &Utf8Path,
        root: TextRange,
        body: TextRange,
        targets: &HashMap<String, crate::FixtureId>,
    ) -> (Vec<(TextRange, crate::FixtureId)>, bool) {
        if targets.is_empty() {
            return (Vec::new(), false);
        }
        self.with_names(path, root, body, |query| {
            let references = query
                .names
                .iter()
                .filter_map(|name| {
                    targets
                        .get(name.id.as_str())
                        .filter(|_| query.binding(name) == Some(query.root))
                        .map(|fixture| (name.range(), fixture.clone()))
                })
                .collect();
            Some((
                references,
                query.unsupported(&targets.keys().cloned().collect()),
            ))
        })
        .unwrap_or_else(|| (Vec::new(), true))
    }

    /// Whether a parameter rename could capture an enclosing name use.
    pub(super) fn contains_name(
        &self,
        path: &Utf8Path,
        root: TextRange,
        body: TextRange,
        name: &str,
    ) -> bool {
        self.with_names(path, root, body, |query| {
            Some(query.names.iter().any(|use_| {
                use_.id == name
                    && query.binding(use_).is_none_or(|binding| {
                        binding == query.root
                            || query
                                .scope_range(binding)
                                .is_none_or(|range| !query.range.contains_range(range))
                    })
            }))
        })
        .unwrap_or(true)
    }

    /// Checks capture in child scopes, including comprehensions and lambdas.
    pub(super) fn nested_conflict(
        &self,
        path: &Utf8Path,
        root: TextRange,
        body: TextRange,
        old: &str,
        new: &str,
    ) -> bool {
        self.with_names(path, root, body, |query| {
            Some(query.names.iter().any(|name| {
                if name.id != old || query.binding(name) != Some(query.root) {
                    return false;
                }
                let Some(scope) = query
                    .index
                    .try_expression_scope_id(&ruff_python_ast::ExprRef::Name(name))
                else {
                    return true;
                };
                query
                    .index
                    .visible_ancestor_scopes(scope)
                    .take_while(|(scope, _)| *scope != query.root)
                    .any(|(scope, _)| {
                        query
                            .index
                            .place_table(scope)
                            .symbol_by_name(new)
                            .is_some_and(ty_python_core::symbol::Symbol::is_bound)
                    })
            }))
        })
        .unwrap_or(true)
    }

    /// Conservatively refuses redirects and class namespaces during fixture rename.
    pub(super) fn unsupported_bindings(
        &self,
        path: &Utf8Path,
        root: TextRange,
        names: &HashSet<String>,
    ) -> bool {
        self.with_names(path, root, TextRange::default(), |query| {
            Some(query.unsupported(names))
        })
        .unwrap_or(true)
    }

    /// Uses ty's symbol table rather than scanning binding constructs independently.
    pub(super) fn local_bindings(&self, path: &Utf8Path, root: TextRange) -> HashSet<String> {
        self.with_names(path, root, TextRange::default(), |query| {
            Some(
                query
                    .index
                    .place_table(query.root)
                    .symbols()
                    .filter(|symbol| symbol.is_bound())
                    .map(|symbol| symbol.name().to_string())
                    .collect(),
            )
        })
        .unwrap_or_default()
    }

    /// Returns final module bindings without reimplementing Python assignment/control-flow rules.
    pub(super) fn bindings(&self, path: &Utf8Path) -> Option<BTreeMap<String, Binding>> {
        self.with_database(|db| {
            let file = system_path_to_file(db, SystemPath::new(path.as_str())).ok()?;
            let program_file = db.program_file(file);
            let scope = global_scope(db, program_file);
            let table = place_table(db, scope);
            let map = use_def_map(db, scope);
            let ast = parsed_module(db, program_file.python_file(db)).load(db);
            Some(
                map.all_end_of_scope_symbol_bindings()
                    .filter_map(|(symbol, mut bindings)| {
                        if !table.symbol(symbol).is_bound() {
                            return None;
                        }
                        let reaching = bindings.by_ref().collect::<Vec<_>>();
                        let definitions = reaching
                            .iter()
                            .filter_map(|binding| binding.binding.definition())
                            .collect::<Vec<_>>();
                        let binding = if !definitions.is_empty()
                            && definitions
                                .iter()
                                .all(|definition| definition.kind(db).as_star_import().is_some())
                        {
                            Binding::Wildcard
                        } else if let [binding] = reaching.as_slice()
                            && let Some(definition) = binding.binding.definition()
                        {
                            Binding::Definition(definition.full_range(db, &ast).range())
                        } else {
                            Binding::Unknown
                        };
                        Some((table.symbol(symbol).name().to_string(), binding))
                    })
                    .collect(),
            )
        })
    }

    /// Delegates literal export-list validation, including mutations, to ty.
    pub(super) fn export_names(&self, path: &Utf8Path) -> Option<Vec<String>> {
        self.with_database(|db| {
            let file = system_path_to_file(db, SystemPath::new(path.as_str())).ok()?;
            let names =
                ty_python_core::static_dunder_all::static_dunder_all(db, db.program_file(file))?;
            Some(names.iter().map(ToString::to_string).collect())
        })
    }

    /// Follows import/reexport identities through ty's module resolver.
    pub(super) fn import_targets(
        &self,
        path: &Utf8Path,
        import_range: TextRange,
        name: &str,
    ) -> Option<Vec<(Utf8PathBuf, TextRange)>> {
        self.with_database(|db| {
            let file = system_path_to_file(db, SystemPath::new(path.as_str())).ok()?;
            let program_file = db.program_file(file);
            let ast = parsed_module(db, program_file.python_file(db)).load(db);
            let import = ast
                .syntax()
                .body
                .iter()
                .find_map(|statement| match statement {
                    Stmt::ImportFrom(import) if import.range() == import_range => Some(import),
                    _ => None,
                })?;
            let model = SemanticModel::new(db, program_file);
            model.resolve_module(
                import
                    .module
                    .as_ref()
                    .map(ruff_python_ast::Identifier::as_str),
                import.level,
            )?;
            Some(
                definitions_for_imported_symbol(
                    &model,
                    import,
                    name,
                    ImportAliasResolution::ResolveAliases,
                )
                .into_iter()
                .filter_map(|definition| {
                    let range = definition.focus_range(db);
                    let path = range.file().path(db).as_system_path()?.as_str();
                    Some((Utf8PathBuf::from(path), range.range()))
                })
                .collect(),
            )
        })
    }

    /// Infers an expression using ty's own AST, never a separately parsed node identity.
    fn expression_type(&self, path: &Utf8Path, range: TextRange) -> Option<String> {
        self.with_database(|db| {
            let file = system_path_to_file(db, SystemPath::new(path.as_str())).ok()?;
            let program_file = db.program_file(file);
            let ast = parsed_module(db, program_file.python_file(db)).load(db);
            let node = ruff_python_ast::find_node::covering_node(ast.syntax().into(), range)
                .find_first(|node| node.as_expr_ref().is_some())
                .ok()?;
            let expression = node.node().as_expr_ref()?;
            let model = SemanticModel::new(db, program_file);
            Self::type_label(&model, expression)
        })
    }

    fn type_label(
        model: &SemanticModel<'_>,
        expression: ruff_python_ast::ExprRef<'_>,
    ) -> Option<String> {
        let db = model.db();
        if let ruff_python_ast::ExprRef::StringLiteral(string) = expression
            && let Some((parsed, model)) = model.enter_string_annotation(string)
        {
            return Self::type_label(&model, parsed.syntax().body.as_ref().into());
        }
        let ty = expression.inferred_type(model)?;
        let environment = model.program_environment();
        if matches!(ty, Type::LiteralValue(literal) if !literal.is_enum())
            && let Some(TypeDefinition::StaticClass(definition)) = ty.definition(db, &environment)
        {
            return definition.name(db);
        }
        (!ty.is_unknown()).then(|| ty.display(db, &environment).to_string())
    }

    /// Returns the injected value type, using ty for annotation and expression inference.
    fn fixture_value_type(&self, path: &Utf8Path, function: &StmtFunctionDef) -> Option<String> {
        if let Some(annotation) = &function.returns {
            let invalid = self.with_database(|db| {
                let file = system_path_to_file(db, SystemPath::new(path.as_str())).ok()?;
                Some(db.check_file(file).iter().any(|diagnostic| {
                    matches!(
                        diagnostic.id().as_str(),
                        "invalid-type-form"
                            | "invalid-type-arguments"
                            | "not-subscriptable"
                            | "unresolved-reference"
                    ) && diagnostic
                        .primary_span()
                        .and_then(|span| span.range())
                        .is_some_and(|range| annotation.range().contains_range(range))
                }))
            })?;
            if invalid {
                return None;
            }
            if crate::fixture::fixture_implementation_range(function) != function.name.range() {
                return self.generator_value_type(path, annotation);
            }
            return self.expression_type(path, annotation.range());
        }
        // ty infers expressions, but does not infer unannotated function return signatures.
        // Until that API exists, expose only straight-line fixture results we can prove.
        if function.body.iter().any(|statement| {
            !matches!(
                statement,
                Stmt::Assign(_)
                    | Stmt::AnnAssign(_)
                    | Stmt::AugAssign(_)
                    | Stmt::Expr(_)
                    | Stmt::Return(_)
                    | Stmt::Pass(_)
            )
        }) {
            return None;
        }
        let generator =
            crate::fixture::fixture_implementation_range(function) != function.name.range();
        let values = function
            .body
            .iter()
            .filter_map(|statement| match statement {
                Stmt::Return(return_) if !generator => return_.value.as_deref().map(Ranged::range),
                Stmt::Expr(expression) => match expression.value.as_ref() {
                    ruff_python_ast::Expr::Yield(yield_) => {
                        yield_.value.as_deref().map(Ranged::range)
                    }
                    _ => None,
                },
                _ => None,
            });
        let mut inferred = None;
        for range in values {
            let value = self.expression_type(path, range)?;
            if inferred.as_ref().is_some_and(|previous| previous != &value) {
                return None;
            }
            inferred = Some(value);
        }
        inferred
    }

    /// Karva injects a generator's yielded value rather than the iterator object.
    fn generator_value_type(&self, path: &Utf8Path, annotation: &Expr) -> Option<String> {
        self.with_database(|db| {
            let file = system_path_to_file(db, SystemPath::new(path.as_str())).ok()?;
            let program_file = db.program_file(file);
            let ast = parsed_module(db, program_file.python_file(db)).load(db);
            let node =
                ruff_python_ast::find_node::covering_node(ast.syntax().into(), annotation.range())
                    .find_first(|node| node.as_expr_ref().is_some())
                    .ok()?;
            let expression = node.node().as_expr_ref()?;
            let model = SemanticModel::new(db, program_file);
            if let ruff_python_ast::ExprRef::StringLiteral(string) = expression {
                let (parsed, model) = model.enter_string_annotation(string)?;
                return Self::generator_annotation(&model, parsed.syntax().body.as_ref().into());
            }
            Self::generator_annotation(&model, expression)
        })
    }

    fn generator_annotation(
        model: &SemanticModel<'_>,
        annotation: ruff_python_ast::ExprRef<'_>,
    ) -> Option<String> {
        let ruff_python_ast::ExprRef::Subscript(subscript) = annotation else {
            return None;
        };
        let db = model.db();
        let ty = subscript.value.inferred_type(model)?;
        let TypeDefinition::StaticClass(definition) =
            ty.definition(db, &model.program_environment())?
        else {
            return None;
        };
        definition.file(db).path(db).as_vendored_path()?;
        if !matches!(
            definition.name(db)?.as_str(),
            "Iterator" | "Generator" | "AsyncIterator" | "AsyncGenerator"
        ) {
            return None;
        }
        Self::type_label(model, annotation)?;
        let value = match subscript.slice.as_ref() {
            Expr::Tuple(tuple) => tuple.elts.first()?,
            value => value,
        };
        Self::type_label(model, value.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karva_collector::{CollectionSettings, collect_source};
    use ruff_python_ast::PythonVersion;

    #[test]
    fn incremental_cache_preserves_old_source_views() {
        let root = Utf8Path::new("/project");
        let path = root.join("test_value.py");
        let settings = CollectionSettings {
            python_version: PythonVersion::PY314,
            test_function_prefix: "test",
            respect_ignore_files: true,
            collect_fixtures: true,
            collect_doctests: false,
        };
        let collect = |value: &str| {
            Arc::new(
                collect_source(
                    &path,
                    root,
                    format!(
                        "from karva import fixture\n@fixture\ndef sample():\n    return {value}\n"
                    ),
                    &settings,
                    &[],
                )
                .expect("source"),
            )
        };
        let first = collect("len('hello')");
        let second = collect("True");
        let cache = Arc::default();
        let old = PythonSemantics::with_cache(
            root,
            &BTreeMap::from([(path.clone(), first.clone())]),
            settings.python_version,
            Arc::clone(&cache),
        );
        let new = PythonSemantics::with_cache(
            root,
            &BTreeMap::from([(path.clone(), second.clone())]),
            settings.python_version,
            cache,
        );
        assert_eq!(
            old.fixture_value_type(&path, &first.fixture_function_defs[0])
                .as_deref(),
            Some("int")
        );
        assert_eq!(
            new.fixture_value_type(&path, &second.fixture_function_defs[0])
                .as_deref(),
            Some("bool")
        );
        assert_eq!(
            old.fixture_value_type(&path, &first.fixture_function_defs[0])
                .as_deref(),
            Some("int")
        );
    }

    #[rstest::rstest]
    #[case("return True", Some("bool"))]
    #[case("value = 1 + 2\n    return value", Some("int"))]
    #[case("return len('hello')", Some("int"))]
    #[case("return missing()", None)]
    #[case("yield 'hello'", Some("str"))]
    #[case("yield 'hello'\n    return 1", Some("str"))]
    #[case("if missing():\n        return 1\n    return 'hello'", None)]
    fn infers_fixture_expressions(#[case] body: &str, #[case] expected: Option<&str>) {
        let source = format!("from karva import fixture\n@fixture\ndef value():\n    {body}\n");
        let root = Utf8Path::new("/project");
        let path = root.join("test_value.py");
        let settings = CollectionSettings {
            python_version: PythonVersion::PY314,
            test_function_prefix: "test",
            respect_ignore_files: true,
            collect_fixtures: true,
            collect_doctests: false,
        };
        let module = Arc::new(collect_source(&path, root, source, &settings, &[]).expect("source"));
        let modules = BTreeMap::from([(path.clone(), Arc::clone(&module))]);
        let semantics = PythonSemantics::new(root, &modules, settings.python_version);
        assert_eq!(
            semantics
                .fixture_value_type(&path, &module.fixture_function_defs[0])
                .as_deref(),
            expected
        );
    }

    #[rstest::rstest]
    #[case("int", "return 1", Some("int"))]
    #[case("'int'", "return 1", Some("int"))]
    #[case("'Iterator[int]'", "yield 1", Some("int"))]
    #[case("list[str]", "return []", Some("list[str]"))]
    #[case("Iterator[int]", "yield 1", Some("int"))]
    #[case("Missing", "return 1", None)]
    #[case("int", "pass", Some("int"))]
    #[case("Iterator[int, str]", "yield 1", None)]
    fn resolves_annotations(
        #[case] annotation: &str,
        #[case] body: &str,
        #[case] expected: Option<&str>,
    ) {
        let source = format!(
            "from typing import Iterator\nfrom karva import fixture\n@fixture\ndef value() -> {annotation}:\n    {body}\n"
        );
        let root = Utf8Path::new("/project");
        let path = root.join("test_value.py");
        let settings = CollectionSettings {
            python_version: PythonVersion::PY314,
            test_function_prefix: "test",
            respect_ignore_files: true,
            collect_fixtures: true,
            collect_doctests: false,
        };
        let module = Arc::new(collect_source(&path, root, source, &settings, &[]).expect("source"));
        let modules = BTreeMap::from([(path.clone(), Arc::clone(&module))]);
        let semantics = PythonSemantics::new(root, &modules, settings.python_version);
        assert_eq!(
            semantics
                .fixture_value_type(&path, &module.fixture_function_defs[0])
                .as_deref(),
            expected
        );
    }
}
