//! Incremental, source-only Python analysis shared by the collector and IDE.
//!
//! Source snapshots include unsaved documents and use an isolated memory filesystem. Queries
//! cannot execute Python or read unrelated files. The optional feature keeps ty out of runtime
//! consumers that only need Karva's names, versions, and syntax-level collection.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use camino::{Utf8Path, Utf8PathBuf};
use ruff_db::files::{File, system_path_to_file};
use ruff_db::parsed::{ParsedModuleRef, parsed_module};
use ruff_db::system::{InMemorySystem, MemoryFileSystem, SystemPath, SystemPathBuf};
use ruff_python_ast::visitor::{self, Visitor};
use ruff_python_ast::{Expr, ExprName, PythonVersion, Stmt, StmtFunctionDef};
use ruff_ranged_value::RangedValue;
use ruff_text_size::{Ranged, TextRange, TextSize};
use ty_project::metadata::Options;
use ty_project::metadata::options::EnvironmentOptions;
use ty_project::{ProjectDatabase, ProjectMetadata};
use ty_python_core::definition::DefinitionKind;
use ty_python_core::scope::{FileScopeId, ScopeKind};
use ty_python_core::{global_scope, place_table, semantic_index, use_def_map};
use ty_python_semantic::types::ide_support::{
    ImportAliasResolution, definitions_for_imported_symbol,
};
use ty_python_semantic::types::{Type, TypeDefinition};
use ty_python_semantic::{Db, HasType, SemanticModel};

use crate::{DecoratorBindings, KnownBinding};

/// Immutable source inputs and a shared incremental query cache.
#[derive(Debug)]
pub struct PythonSourceSnapshot<S> {
    cache: Arc<PythonSourceCache<S>>,
    root: Utf8PathBuf,
    modules: Arc<BTreeMap<Utf8PathBuf, Arc<S>>>,
    python_version: PythonVersion,
}

/// ty database retained across source snapshots, without retaining Salsa query clones.
///
/// Queries are serialized and activate their snapshot before reading. Older snapshots remain
/// valid after an edit; callers cannot hold database references outside the query closure.
#[derive(Debug)]
pub struct PythonSourceCache<S> {
    project: Mutex<Option<SemanticProject<S>>>,
}

impl<S> Default for PythonSourceCache<S> {
    fn default() -> Self {
        Self {
            project: Mutex::new(None),
        }
    }
}

#[derive(Debug)]
struct SemanticProject<S> {
    db: ProjectDatabase,
    filesystem: MemoryFileSystem,
    root: Utf8PathBuf,
    python_version: PythonVersion,
    modules: Arc<BTreeMap<Utf8PathBuf, Arc<S>>>,
}

/// One final Python binding, as determined by ty's control-flow analysis.
pub enum Binding {
    /// Exactly one reaching declaration in the module.
    Definition(TextRange),
    /// Multiple, missing or conditional declarations cannot be selected safely.
    Unknown,
    /// A name contributed solely by a wildcard import; Karva checks its export policy separately.
    Wildcard,
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

/// Resolves import and assignment aliases at their use, using ty's control-flow facts.
struct DecoratorQuery<'db, 'ast> {
    db: &'db dyn Db,
    index: &'db ty_python_core::SemanticIndex<'db>,
    ast: &'ast ParsedModuleRef,
    active: HashSet<TextRange>,
}

impl DecoratorQuery<'_, '_> {
    fn expression(&mut self, expression: &Expr) -> Option<KnownBinding> {
        if !self.active.insert(expression.range()) {
            return None;
        }
        let binding = match expression {
            Expr::Name(name) => self.name(name),
            Expr::Attribute(attribute) => self
                .expression(&attribute.value)
                .and_then(|namespace| namespace.attribute(attribute.attr.as_str())),
            _ => None,
        };
        self.active.remove(&expression.range());
        binding
    }

    fn name(&mut self, name: &ExprName) -> Option<KnownBinding> {
        let expression = ruff_python_ast::ExprRef::Name(name);
        let scope = self.index.try_expression_scope_id(&expression)?;
        let use_id = self.index.try_expression_use_id(expression)?;
        let bindings = self
            .index
            .use_def_map(scope)
            .bindings_at_use(use_id)
            .map(|binding| binding.binding)
            .collect::<Vec<_>>();
        let mut result = None;
        for binding in bindings {
            let Some(definition) = binding.definition() else {
                // Preserve only Karva's historical bare decorator shorthand when never bound.
                let symbol = self
                    .index
                    .place_table(scope)
                    .symbol_by_name(name.id.as_str())?;
                return (!symbol.is_bound())
                    .then(|| DecoratorBindings::default().get(name.id.as_str()))
                    .flatten();
            };
            let value = match definition.kind(self.db) {
                DefinitionKind::Import(import) => {
                    let alias = import.alias(self.ast);
                    let module = if alias.asname.is_some() {
                        alias.name.as_str()
                    } else {
                        alias.name.as_str().split('.').next()?
                    };
                    KnownBinding::from_module(module)
                }
                DefinitionKind::ImportFrom(import) => {
                    let statement = import.import(self.ast);
                    (statement.level == 0).then_some(()).and_then(|()| {
                        KnownBinding::from_import(
                            statement.module.as_ref()?.as_str(),
                            import.alias(self.ast).name.as_str(),
                        )
                    })
                }
                DefinitionKind::Assignment(assignment) if assignment.unpack().is_none() => {
                    self.expression(assignment.value(self.ast))
                }
                DefinitionKind::AnnotatedAssignment(assignment) => assignment
                    .value(self.ast)
                    .and_then(|value| self.expression(value)),
                DefinitionKind::NamedExpression(named) => {
                    self.expression(&named.node(self.ast).value)
                }
                _ => None,
            }?;
            if result.is_some_and(|previous| previous != value) {
                return None;
            }
            result = Some(value);
        }
        result
    }
}

impl<S: AsRef<str>> PythonSourceSnapshot<S> {
    /// Retains shared source objects without copying their text or constructing a database yet.
    pub fn new(
        root: Utf8PathBuf,
        modules: Arc<BTreeMap<Utf8PathBuf, Arc<S>>>,
        python_version: PythonVersion,
        cache: Arc<PythonSourceCache<S>>,
    ) -> Self {
        Self {
            cache,
            root,
            modules,
            python_version,
        }
    }

    /// Checks the source/version contract before a consumer mixes syntax with semantic results.
    pub fn contains_source(&self, path: &Utf8Path, source: &str, version: PythonVersion) -> bool {
        self.python_version == version
            && self
                .modules
                .get(path)
                .is_some_and(|module| module.as_ref().as_ref() == source)
    }

    /// Resolves each top-level function's decorator names at their actual source uses.
    ///
    /// The environments contain only names used by decorators. Missing or ambiguous bindings
    /// remain unknown; assignment aliases are followed without importing or executing Python.
    pub fn decorator_bindings(
        &self,
        path: &Utf8Path,
    ) -> Option<BTreeMap<TextSize, DecoratorBindings>> {
        self.with_database(|db| {
            let file = system_path_to_file(db, SystemPath::new(path.as_str())).ok()?;
            let program_file = db.program_file(file);
            let ast = parsed_module(db, program_file.python_file(db)).load(db);
            let mut query = DecoratorQuery {
                db,
                index: semantic_index(db, program_file),
                ast: &ast,
                active: HashSet::new(),
            };
            Some(
                ast.syntax()
                    .body
                    .iter()
                    .filter_map(|statement| {
                        let Stmt::FunctionDef(function) = statement else {
                            return None;
                        };
                        let mut names = Names {
                            names: Vec::new(),
                            range: function.range(),
                        };
                        for decorator in &function.decorator_list {
                            names.range = decorator.expression.range();
                            names.visit_expr(&decorator.expression);
                        }
                        let bindings = names
                            .names
                            .into_iter()
                            .map(|name| {
                                (
                                    name.id.to_string(),
                                    query.name(name).unwrap_or(KnownBinding::Unknown),
                                )
                            })
                            .collect();
                        Some((
                            function.range().start(),
                            DecoratorBindings::resolved(bindings),
                        ))
                    })
                    .collect(),
            )
        })
    }

    fn create_project(&self) -> std::io::Result<SemanticProject<S>> {
        let root = &self.root;
        let modules = &self.modules;
        let python_version = self.python_version;
        let filesystem = MemoryFileSystem::with_current_directory(SystemPath::new(root.as_str()));
        for (path, module) in modules.iter() {
            filesystem.write_file_all(SystemPath::new(path.as_str()), module.as_ref().as_ref())?;
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

    /// Executes a query against this source generation; invalid inputs or poisoned locks return None.
    ///
    /// ponytail: one database lock serializes queries; use independent project caches if measured
    /// contention justifies partitioning. No Salsa clones survive this lock.
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
                    Arc::ptr_eq(previous, module)
                        || previous.as_ref().as_ref() == module.as_ref().as_ref()
                }) {
                    continue;
                }
                project
                    .filesystem
                    .write_file_all(SystemPath::new(path.as_str()), module.as_ref().as_ref())
                    .ok()?;
                File::sync_path(&mut project.db, SystemPath::new(path.as_str()));
            }
            project.modules = Arc::clone(&self.modules);
        }
        query(&project.db)
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

    /// Connects tracked name uses to bindings in the requested lexical scope.
    pub fn references<V: Clone>(
        &self,
        path: &Utf8Path,
        root: TextRange,
        body: TextRange,
        targets: &HashMap<String, V>,
    ) -> (Vec<(TextRange, V)>, bool) {
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
                        .map(|target| (name.range(), target.clone()))
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
    pub fn contains_name(
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
    pub fn nested_conflict(
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

    /// Reports global/nonlocal redirects and class namespaces that require consumer-specific policy.
    pub fn unsupported_bindings(
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
    pub fn local_bindings(&self, path: &Utf8Path, root: TextRange) -> HashSet<String> {
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
    pub fn bindings(&self, path: &Utf8Path) -> Option<BTreeMap<String, Binding>> {
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
    pub fn export_names(&self, path: &Utf8Path) -> Option<Vec<String>> {
        self.with_database(|db| {
            let file = system_path_to_file(db, SystemPath::new(path.as_str())).ok()?;
            let names =
                ty_python_core::static_dunder_all::static_dunder_all(db, db.program_file(file))?;
            Some(names.iter().map(ToString::to_string).collect())
        })
    }

    /// Follows import/reexport identities through ty's module resolver.
    pub fn import_targets(
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
        self.expression_label(path, range, false)
    }

    fn expression_label(
        &self,
        path: &Utf8Path,
        range: TextRange,
        annotation: bool,
    ) -> Option<String> {
        self.with_database(|db| {
            let file = system_path_to_file(db, SystemPath::new(path.as_str())).ok()?;
            let program_file = db.program_file(file);
            let ast = parsed_module(db, program_file.python_file(db)).load(db);
            let node = ruff_python_ast::find_node::covering_node(ast.syntax().into(), range)
                .find_first(|node| node.as_expr_ref().is_some())
                .ok()?;
            let expression = node.node().as_expr_ref()?;
            let model = SemanticModel::new(db, program_file);
            Self::type_label(&model, expression, annotation)
        })
    }

    fn type_label(
        model: &SemanticModel<'_>,
        expression: ruff_python_ast::ExprRef<'_>,
        annotation: bool,
    ) -> Option<String> {
        let db = model.db();
        if annotation
            && let ruff_python_ast::ExprRef::StringLiteral(string) = expression
            && let Some((parsed, model)) = model.enter_string_annotation(string)
        {
            return Self::type_label(&model, parsed.syntax().body.as_ref().into(), true);
        }
        let ty = expression.inferred_type(model)?;
        let environment = model.program_environment();
        if !annotation
            && matches!(ty, Type::LiteralValue(literal) if !literal.is_enum())
            && let Some(TypeDefinition::StaticClass(definition)) = ty.definition(db, &environment)
        {
            return definition.name(db);
        }
        (!ty.is_unknown()).then(|| ty.display(db, &environment).to_string())
    }

    /// Resolves an annotation or conservatively infers a straight-line function's output value.
    ///
    /// Set `yields` when the consumer wants a generator's yielded value rather than its iterator.
    /// Syntax must come from this source snapshot. Branching unannotated functions remain unknown
    /// because ty's public API does not infer their return signatures.
    pub fn function_value_type(
        &self,
        path: &Utf8Path,
        function: &StmtFunctionDef,
        yields: bool,
    ) -> Option<String> {
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
            if yields {
                return self.generator_value_type(path, annotation);
            }
            return self.expression_label(path, annotation.range(), true);
        }
        // ty infers expressions, but does not infer unannotated function return signatures.
        // Until that API exists, expose only straight-line results we can prove.
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
        let generator = yields;
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

    /// Extracts yielded values from known standard-library iterator/generator annotations.
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
        Self::type_label(model, annotation, true)?;
        let value = match subscript.slice.as_ref() {
            Expr::Tuple(tuple) => tuple.elts.first()?,
            value => value,
        };
        Self::type_label(model, value.into(), true)
    }
}
