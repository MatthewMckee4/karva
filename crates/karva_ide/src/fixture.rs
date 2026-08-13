#![expect(
    clippy::redundant_pub_crate,
    reason = "the crate root re-exports fixture analysis across this private module"
)]

use std::collections::{HashMap, HashSet};

use camino::Utf8PathBuf;
use karva_collector::{CollectedModule, ModuleType};
use ruff_python_ast::visitor::source_order::{self, SourceOrderVisitor};
use ruff_python_ast::{Expr, Stmt, StmtFunctionDef};
use ruff_text_size::{Ranged, TextRange};

use crate::{DiagnosticCode, RelatedInformation, SourceDiagnostic, SourceLocation};

/// Stable identity for a fixture provider.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FixtureId {
    /// File containing the fixture.
    pub path: Utf8PathBuf,

    /// Function-name range anchoring the fixture declaration.
    pub(super) range: TextRange,
}

/// Runtime lifetime of a fixture value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum FixtureScope {
    /// Recreated for every test attempt.
    #[default]
    Function,

    /// Shared by tests in one module.
    Module,

    /// Shared by modules in one package.
    Package,

    /// Shared by the complete worker session.
    Session,
}

impl FixtureScope {
    /// Returns whether this fixture may depend on `dependency`.
    const fn can_use(self, dependency: Self) -> bool {
        self as u8 <= dependency as u8
    }

    /// Returns the configuration spelling of the scope.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Module => "module",
            Self::Package => "package",
            Self::Session => "session",
        }
    }
}

impl TryFrom<&str> for FixtureScope {
    type Error = ();

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "function" => Ok(Self::Function),
            "module" => Ok(Self::Module),
            "package" => Ok(Self::Package),
            "session" => Ok(Self::Session),
            _ => Err(()),
        }
    }
}

/// Result of resolving one fixture reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum FixtureResolution {
    /// The reference resolves to a source fixture.
    Resolved(FixtureId),

    /// The reference resolves to a Karva-provided fixture.
    Builtin,

    /// Definitions with this name were rejected.
    Rejected(Vec<FixtureId>),

    /// No visible provider exists.
    Missing,

    /// Dynamic imports or decorator metadata prevent a definite answer.
    Unknown,
}

/// One fixture reference from a function parameter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FixtureReference {
    /// Requested fixture name.
    pub(super) name: String,

    /// Range of the parameter name.
    pub(super) range: TextRange,

    /// Static lookup result.
    pub(super) resolution: FixtureResolution,
}

/// A statically understood fixture declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FixtureDefinition {
    /// Stable source identity.
    pub(super) id: FixtureId,

    /// Public fixture name after literal decorator renaming.
    pub(super) name: String,

    /// Python function name.
    defining_name: String,

    /// Function-name source range.
    pub(super) name_range: TextRange,

    /// Source range of the provider's first yield expression, or its function
    /// name when the provider is not a generator.
    pub(super) implementation_range: TextRange,

    /// Source range representing the public fixture name.
    ///
    /// This is the function name for ordinary fixtures, the string contents
    /// for a single literal `name=`, or the complete expression for implicit
    /// string concatenation.
    pub(super) public_name_range: TextRange,

    /// Range that can be replaced with another public fixture name.
    ///
    /// Implicitly concatenated string literals have no syntax-preserving
    /// single replacement range.
    pub(super) public_name_edit_range: Option<TextRange>,

    /// Source signature of the provider function.
    pub(super) signature: String,

    /// Leading function docstring, when statically available.
    pub(super) docstring: Option<String>,

    /// Statically known scope, or `None` for dynamic scope metadata.
    pub(super) scope: Option<FixtureScope>,

    /// Statically known autouse value, or `None` for dynamic metadata.
    pub(super) auto_use: Option<bool>,

    /// Fixture dependencies declared as non-variadic parameters.
    pub(super) dependencies: Vec<FixtureReference>,
}

#[derive(Clone, Debug)]
struct ParsedFixture {
    definition: FixtureDefinition,
    public_name_known: bool,
    invalid: Vec<InvalidMetadata>,
}

/// Source ranges used to locate and safely replace one public fixture name.
#[derive(Clone, Copy, Debug)]
struct PublicNameRanges {
    occurrence: TextRange,
    edit: Option<TextRange>,
}

#[derive(Clone, Debug)]
struct FixtureProvider {
    definitions: Vec<FixtureDefinition>,
    by_name: HashMap<String, FixtureId>,
    rejected: HashMap<String, Vec<FixtureId>>,
    unknown: bool,
    diagnostics: Vec<SourceDiagnostic>,
}

#[derive(Clone, Debug)]
struct InvalidMetadata {
    range: TextRange,
    message: String,
}

/// Static metadata for a fixture bundled with Karva.
#[derive(Clone, Copy)]
pub(super) struct BuiltinFixture {
    /// Public fixture name.
    name: &'static str,

    /// Runtime fixture scope.
    pub(super) scope: FixtureScope,

    /// Source-like signature shown by editor features.
    pub(super) signature: &'static str,

    /// User-facing description derived from the bundled implementation.
    pub(super) docstring: &'static str,
}

const BUILTIN_FIXTURES: &[BuiltinFixture] = &[
    BuiltinFixture {
        name: "monkeypatch",
        scope: FixtureScope::Function,
        signature: "monkeypatch() -> Generator[MockEnv, None, None]",
        docstring: "Fixture that provides a `MockEnv` for patching during a test.",
    },
    BuiltinFixture {
        name: "capsys",
        scope: FixtureScope::Function,
        signature: "capsys() -> Generator[_CapsysFixture, None, None]",
        docstring: "Capture writes to `sys.stdout` and `sys.stderr`.",
    },
    BuiltinFixture {
        name: "capfd",
        scope: FixtureScope::Function,
        signature: "capfd() -> Generator[_CapfdFixture, None, None]",
        docstring: "Capture writes to file descriptors 1 and 2 as strings.",
    },
    BuiltinFixture {
        name: "capsysbinary",
        scope: FixtureScope::Function,
        signature: "capsysbinary() -> Generator[_CapsysBinaryFixture, None, None]",
        docstring: "Capture writes to `sys.stdout` and `sys.stderr` as bytes.",
    },
    BuiltinFixture {
        name: "capfdbinary",
        scope: FixtureScope::Function,
        signature: "capfdbinary() -> Generator[_CapsysBinaryFixture, None, None]",
        docstring: "Capture writes to `sys.stdout` and `sys.stderr` as bytes (fd-level alias).",
    },
    BuiltinFixture {
        name: "caplog",
        scope: FixtureScope::Function,
        signature: "caplog() -> Generator[_CapLog, None, None]",
        docstring: "Capture log records emitted during a test.",
    },
    BuiltinFixture {
        name: "tmp_path",
        scope: FixtureScope::Function,
        signature: "tmp_path(tmp_path_factory: TempPathFactory) -> Path",
        docstring: "Provide a temporary directory as a `pathlib.Path` object.",
    },
    BuiltinFixture {
        name: "temp_path",
        scope: FixtureScope::Function,
        signature: "temp_path(tmp_path_factory: TempPathFactory) -> Path",
        docstring: "Alias for `tmp_path`.",
    },
    BuiltinFixture {
        name: "temp_dir",
        scope: FixtureScope::Function,
        signature: "temp_dir(tmp_path_factory: TempPathFactory) -> Path",
        docstring: "Alias for `tmp_path`.",
    },
    BuiltinFixture {
        name: "tmpdir",
        scope: FixtureScope::Function,
        signature: "tmpdir(tmp_path_factory: TempPathFactory) -> Path",
        docstring: "Provide a temporary directory as a `pathlib.Path` object.",
    },
    BuiltinFixture {
        name: "tmp_path_factory",
        scope: FixtureScope::Session,
        signature: "tmp_path_factory() -> TempPathFactory",
        docstring: "Session-scoped factory for creating numbered temporary directories.",
    },
    BuiltinFixture {
        name: "tmpdir_factory",
        scope: FixtureScope::Session,
        signature: "tmpdir_factory() -> TempPathFactory",
        docstring: "Session-scoped factory for creating numbered temporary directories.",
    },
    BuiltinFixture {
        name: "recwarn",
        scope: FixtureScope::Function,
        signature: "recwarn() -> Generator[WarningsRecorder, None, None]",
        docstring: "Return a `WarningsRecorder` that records warnings raised during a test.",
    },
];

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "completion and navigation consumers land in later stack layers"
    )
)]
pub(super) fn analyze(
    module: &CollectedModule,
    try_import_fixtures: bool,
) -> (Vec<FixtureDefinition>, Vec<SourceDiagnostic>) {
    analyze_modules(module, &[], try_import_fixtures)
}

/// Analyzes a module against configuration providers ordered from the session
/// root toward the nearest package.
pub(super) fn analyze_modules(
    current: &CollectedModule,
    parents: &[&CollectedModule],
    try_import_fixtures: bool,
) -> (Vec<FixtureDefinition>, Vec<SourceDiagnostic>) {
    let mut providers = parents
        .iter()
        .rev()
        .map(|module| parse_provider(module, try_import_fixtures))
        .collect::<Vec<_>>();
    let current_provider = parse_provider(current, try_import_fixtures);
    providers.insert(0, current_provider);

    let definition_order = providers
        .iter()
        .flat_map(|provider| provider.definitions.iter())
        .map(|definition| definition.id.clone())
        .collect::<Vec<_>>();
    let mut definitions = providers
        .iter()
        .flat_map(|provider| provider.definitions.iter().cloned())
        .map(|definition| (definition.id.clone(), definition))
        .collect::<HashMap<_, _>>();

    let resolutions = providers
        .iter()
        .flat_map(|provider| provider.definitions.iter())
        .map(|definition| {
            let resolved = definition
                .dependencies
                .iter()
                .map(|reference| {
                    (
                        reference.name.clone(),
                        resolve_reference(&providers, Some(definition), &reference.name),
                    )
                })
                .collect::<Vec<_>>();
            (definition.id.clone(), resolved)
        })
        .collect::<HashMap<_, _>>();

    for definition in definitions.values_mut() {
        if let Some(resolved) = resolutions.get(&definition.id) {
            for reference in &mut definition.dependencies {
                if let Some((_, resolution)) =
                    resolved.iter().find(|(name, _)| name == &reference.name)
                {
                    reference.resolution = resolution.clone();
                }
            }
        }
    }

    let mut diagnostics = providers
        .iter()
        .flat_map(|provider| provider.diagnostics.iter().cloned())
        .collect::<Vec<_>>();
    let all_definitions = definition_order
        .iter()
        .filter_map(|id| definitions.get(id))
        .collect::<Vec<_>>();
    diagnostics.extend(reference_diagnostics(&all_definitions));
    diagnostics.extend(scope_diagnostics(&all_definitions));
    diagnostics.extend(cycle_diagnostics(&all_definitions));
    diagnostics.extend(test_diagnostics(current, &providers));

    let current_definitions = providers[0]
        .definitions
        .iter()
        .filter_map(|definition| definitions.get(&definition.id).cloned())
        .collect::<Vec<_>>();
    diagnostics.sort_by(|left, right| {
        (
            &left.location.path,
            left.location.range.start(),
            left.code.as_str(),
        )
            .cmp(&(
                &right.location.path,
                right.location.range.start(),
                right.code.as_str(),
            ))
    });
    (current_definitions, diagnostics)
}

/// Visible fixture providers plus conservative completion barriers.
pub(super) struct VisibleFixtures {
    /// Definitions selected before any dynamic provider barrier.
    pub(super) definitions: Vec<FixtureDefinition>,

    /// Rejected names that prevent fallback to later providers.
    pub(super) blocked_names: HashSet<String>,

    /// Whether every provider was statically known, allowing built-in fallback.
    pub(super) builtins_visible: bool,
}

/// Returns fixture definitions that can be selected from the current module.
///
/// Providers follow runtime lookup order. A rejected definition blocks the same
/// name from every later provider, including Karva's built-ins.
pub(super) fn visible_fixtures(
    current: &CollectedModule,
    parents: &[&CollectedModule],
    try_import_fixtures: bool,
) -> VisibleFixtures {
    let mut providers = parents
        .iter()
        .rev()
        .map(|module| parse_provider(module, try_import_fixtures))
        .collect::<Vec<_>>();
    providers.insert(0, parse_provider(current, try_import_fixtures));

    let mut visible = Vec::new();
    let mut names = HashSet::new();
    let mut blocked_names = HashSet::new();
    let mut builtins_visible = true;
    for provider in providers {
        for rejected in provider.rejected.keys() {
            if names.insert(rejected.clone()) {
                blocked_names.insert(rejected.clone());
            }
        }
        for definition in provider.definitions {
            if names.insert(definition.name.clone()) {
                visible.push(definition);
            }
        }
        if provider.unknown {
            builtins_visible = false;
            break;
        }
    }
    VisibleFixtures {
        definitions: visible,
        blocked_names,
        builtins_visible,
    }
}

/// Returns the built-in fixtures exposed by Karva's runtime.
pub(super) fn builtin_fixtures() -> impl Iterator<Item = (&'static str, FixtureScope)> {
    BUILTIN_FIXTURES
        .iter()
        .map(|fixture| (fixture.name, fixture.scope))
}

/// Returns bundled metadata for a built-in fixture name.
pub(super) fn builtin_info(name: &str) -> Option<BuiltinFixture> {
    builtin(name)
}

impl FixtureProvider {
    fn from_parsed(
        parsed: &[ParsedFixture],
        path: &Utf8PathBuf,
        imported_fixtures_unknown: bool,
    ) -> Self {
        let mut diagnostics = Vec::new();
        let rejected = duplicate_fixtures(parsed, path, &mut diagnostics);
        let rejected = parsed
            .iter()
            .filter(|fixture| {
                rejected.contains(&fixture.definition.id) || !fixture.invalid.is_empty()
            })
            .fold(
                HashMap::<String, Vec<FixtureId>>::new(),
                |mut by_name, fixture| {
                    by_name
                        .entry(fixture.definition.name.clone())
                        .or_default()
                        .push(fixture.definition.id.clone());
                    by_name
                },
            );
        diagnostics.extend(invalid_metadata_diagnostics(parsed, path));

        let definitions = parsed
            .iter()
            .filter(|fixture| {
                fixture.public_name_known
                    && fixture.invalid.is_empty()
                    && !rejected
                        .values()
                        .any(|ids| ids.contains(&fixture.definition.id))
            })
            .map(|fixture| fixture.definition.clone())
            .collect::<Vec<_>>();
        let by_name = definitions
            .iter()
            .map(|definition| (definition.name.clone(), definition.id.clone()))
            .collect();

        Self {
            definitions,
            by_name,
            rejected,
            unknown: imported_fixtures_unknown
                || parsed.iter().any(|fixture| !fixture.public_name_known),
            diagnostics,
        }
    }
}

fn parse_provider(module: &CollectedModule, try_import_fixtures: bool) -> FixtureProvider {
    let path = module.path.path().clone();
    let bindings = FixtureBindings::from_statements(&module.module_body);
    let parsed = module
        .fixture_function_defs
        .iter()
        .map(|function| parse_fixture(function, &path, bindings, &module.source_text))
        .collect::<Vec<_>>();
    let imported_fixtures_unknown = (try_import_fixtures
        || module.module_type == ModuleType::Configuration)
        && has_external_imports(&module.module_body);
    FixtureProvider::from_parsed(&parsed, &path, imported_fixtures_unknown)
}

#[derive(Clone, Copy, Debug, Default)]
struct FixtureBindings {
    bare: bool,
    karva: bool,
    pytest: bool,
}

impl FixtureBindings {
    fn from_statements(statements: &[Stmt]) -> Self {
        let mut bindings = Self::default();
        for statement in statements {
            match statement {
                Stmt::Import(import) => {
                    for alias in &import.names {
                        let binding = alias.asname.as_ref().map_or_else(
                            || alias.name.as_str(),
                            ruff_python_ast::Identifier::as_str,
                        );
                        match (alias.name.as_str(), binding) {
                            ("karva", "karva") => bindings.karva = true,
                            ("pytest", "pytest") => bindings.pytest = true,
                            _ => {}
                        }
                    }
                }
                Stmt::ImportFrom(import)
                    if import
                        .module
                        .as_ref()
                        .is_some_and(|module| matches!(module.as_str(), "karva" | "pytest")) =>
                {
                    bindings.bare |= import.names.iter().any(|alias| {
                        alias.name.as_str() == "fixture"
                            && alias
                                .asname
                                .as_ref()
                                .is_none_or(|name| name.as_str() == "fixture")
                    });
                }
                _ => {}
            }
        }
        bindings
    }

    fn is_parametrize_reference(self, expression: &Expr) -> bool {
        match expression {
            Expr::Name(name) => name.id == "parametrize",
            Expr::Attribute(attribute) if attribute.attr.id == "parametrize" => {
                matches!(
                    attribute.value.as_ref(),
                    Expr::Attribute(namespace)
                        if matches!(
                            (namespace.value.as_ref(), namespace.attr.id.as_str()),
                            (Expr::Name(name), "mark") if self.pytest && name.id == "pytest"
                        ) || matches!(
                            (namespace.value.as_ref(), namespace.attr.id.as_str()),
                            (Expr::Name(name), "tags") if self.karva && name.id == "karva"
                        )
                )
            }
            _ => false,
        }
    }

    fn is_use_fixtures_reference(self, expression: &Expr) -> bool {
        let Expr::Attribute(attribute) = expression else {
            return false;
        };
        match attribute.attr.as_str() {
            "use_fixtures" => matches!(
                attribute.value.as_ref(),
                Expr::Attribute(namespace)
                    if self.karva
                        && namespace.attr.as_str() == "tags"
                        && matches!(namespace.value.as_ref(), Expr::Name(name) if name.id == "karva")
            ),
            "usefixtures" => matches!(
                attribute.value.as_ref(),
                Expr::Attribute(namespace)
                    if self.pytest
                        && namespace.attr.as_str() == "mark"
                        && matches!(namespace.value.as_ref(), Expr::Name(name) if name.id == "pytest")
            ),
            _ => false,
        }
    }
}

pub(super) fn is_use_fixtures_reference(module: &CollectedModule, expression: &Expr) -> bool {
    FixtureBindings::from_statements(&module.module_body).is_use_fixtures_reference(expression)
}

/// Returns the replaceable contents of a non-concatenated string literal.
pub(super) fn single_string_content_range(
    expression: &ruff_python_ast::ExprStringLiteral,
) -> Option<TextRange> {
    let [literal] = expression.value.as_slice() else {
        return None;
    };
    Some(literal.content_range())
}

fn parse_fixture(
    function: &StmtFunctionDef,
    path: &Utf8PathBuf,
    bindings: FixtureBindings,
    source: &str,
) -> ParsedFixture {
    let mut public_name = function.name.to_string();
    let mut public_name_ranges = PublicNameRanges {
        occurrence: function.name.range,
        edit: Some(function.name.range),
    };
    let mut public_name_known = false;
    let mut scope = None;
    let mut auto_use = None;
    let mut invalid = Vec::new();

    if let Some(decorator) = function
        .decorator_list
        .iter()
        .find(|decorator| is_fixture_decorator(&decorator.expression, bindings))
    {
        public_name_known = true;
        scope = Some(FixtureScope::Function);
        auto_use = Some(false);
        let Some(call) = (match &decorator.expression {
            Expr::Call(call) => Some(call),
            _ => None,
        }) else {
            return ParsedFixture {
                definition: fixture_definition(
                    function,
                    path,
                    public_name,
                    public_name_ranges,
                    scope,
                    auto_use,
                    source,
                ),
                public_name_known,
                invalid,
            };
        };
        if !call.arguments.args.is_empty() {
            public_name_known = false;
            scope = None;
            auto_use = None;
        }
        for keyword in &call.arguments.keywords {
            let Some(name) = keyword
                .arg
                .as_ref()
                .map(ruff_python_ast::Identifier::as_str)
            else {
                public_name_known = false;
                scope = None;
                auto_use = None;
                continue;
            };
            match name {
                "name" => match &keyword.value {
                    Expr::StringLiteral(value) => {
                        value.value.to_str().clone_into(&mut public_name);
                        let edit = single_string_content_range(value);
                        public_name_ranges = PublicNameRanges {
                            occurrence: edit.unwrap_or_else(|| value.range()),
                            edit,
                        };
                    }
                    Expr::NoneLiteral(_) => {}
                    Expr::NumberLiteral(_) | Expr::BooleanLiteral(_) => {
                        invalid.push(InvalidMetadata {
                            range: keyword.value.range(),
                            message: "Fixture `name` must be a string or `None`".to_owned(),
                        });
                    }
                    _ => public_name_known = false,
                },
                "scope" => match &keyword.value {
                    Expr::StringLiteral(value) => {
                        let value = value.value.to_str();
                        match FixtureScope::try_from(value) {
                            Ok(value) => scope = Some(value),
                            Err(()) => invalid.push(InvalidMetadata {
                                range: keyword.value.range(),
                                message: format!("Invalid fixture scope `{value}`"),
                            }),
                        }
                    }
                    Expr::NumberLiteral(_) | Expr::BooleanLiteral(_) | Expr::NoneLiteral(_) => {
                        invalid.push(InvalidMetadata {
                            range: keyword.value.range(),
                            message: "Fixture `scope` must be a string or callable".to_owned(),
                        });
                    }
                    _ => scope = None,
                },
                "auto_use" | "autouse" => match &keyword.value {
                    Expr::BooleanLiteral(value) => auto_use = Some(value.value),
                    Expr::StringLiteral(_) | Expr::NumberLiteral(_) | Expr::NoneLiteral(_) => {
                        invalid.push(InvalidMetadata {
                            range: keyword.value.range(),
                            message: "Fixture autouse value must be a boolean".to_owned(),
                        });
                    }
                    _ => auto_use = None,
                },
                _ => invalid.push(InvalidMetadata {
                    range: keyword.range,
                    message: format!("Unsupported fixture argument `{name}`"),
                }),
            }
        }
    }

    ParsedFixture {
        definition: fixture_definition(
            function,
            path,
            public_name,
            public_name_ranges,
            scope,
            auto_use,
            source,
        ),
        public_name_known,
        invalid,
    }
}

fn fixture_definition(
    function: &StmtFunctionDef,
    path: &Utf8PathBuf,
    name: String,
    public_name_ranges: PublicNameRanges,
    scope: Option<FixtureScope>,
    auto_use: Option<bool>,
    source: &str,
) -> FixtureDefinition {
    FixtureDefinition {
        id: FixtureId {
            path: path.clone(),
            range: function.name.range,
        },
        name,
        defining_name: function.name.to_string(),
        name_range: function.name.range,
        implementation_range: fixture_implementation_range(function),
        public_name_range: public_name_ranges.occurrence,
        public_name_edit_range: public_name_ranges.edit,
        signature: function_signature(function, source),
        docstring: function_docstring(function),
        scope,
        auto_use,
        dependencies: function
            .parameters
            .iter_non_variadic_params()
            .map(|parameter| FixtureReference {
                name: parameter.parameter.name.to_string(),
                range: parameter.parameter.name.range,
                resolution: FixtureResolution::Unknown,
            })
            .collect(),
    }
}

fn fixture_implementation_range(function: &StmtFunctionDef) -> TextRange {
    let mut visitor = FixtureImplementationVisitor::default();
    source_order::walk_body(&mut visitor, &function.body);
    visitor.implementation_range.unwrap_or(function.name.range)
}

#[derive(Default)]
struct FixtureImplementationVisitor {
    implementation_range: Option<TextRange>,
}

impl SourceOrderVisitor<'_> for FixtureImplementationVisitor {
    fn visit_stmt(&mut self, statement: &'_ Stmt) {
        match statement {
            Stmt::FunctionDef(_) | Stmt::ClassDef(_) => {}
            _ => source_order::walk_stmt(self, statement),
        }
    }

    fn visit_expr(&mut self, expression: &'_ Expr) {
        if self.implementation_range.is_some() {
            return;
        }
        match expression {
            Expr::Yield(_) | Expr::YieldFrom(_) => {
                self.implementation_range = Some(expression.range());
            }
            Expr::Lambda(_) => {}
            _ => source_order::walk_expr(self, expression),
        }
    }
}

fn function_signature(function: &StmtFunctionDef, source: &str) -> String {
    let start = function.range.start().to_usize();
    let end = function
        .body
        .first()
        .map_or(function.range, Ranged::range)
        .start()
        .to_usize();
    source.get(start..end).map_or_else(
        || format!("def {}(...)", function.name),
        |signature| {
            let signature = signature.trim();
            let start = signature
                .find("async def ")
                .or_else(|| signature.find("def "))
                .unwrap_or(0);
            signature[start..].to_owned()
        },
    )
}

fn function_docstring(function: &StmtFunctionDef) -> Option<String> {
    let Stmt::Expr(statement) = function.body.first()? else {
        return None;
    };
    let Expr::StringLiteral(string) = statement.value.as_ref() else {
        return None;
    };
    Some(string.value.to_str().to_owned())
}

fn is_fixture_decorator(expression: &Expr, bindings: FixtureBindings) -> bool {
    match expression {
        Expr::Call(call) => is_fixture_reference(call.func.as_ref(), bindings),
        _ => is_fixture_reference(expression, bindings),
    }
}

fn is_fixture_reference(expression: &Expr, bindings: FixtureBindings) -> bool {
    match expression {
        Expr::Name(name) => bindings.bare && name.id == "fixture",
        Expr::Attribute(attribute) if attribute.attr.id == "fixture" => {
            matches!(
                attribute.value.as_ref(),
                Expr::Name(name)
                    if (bindings.karva && name.id == "karva")
                        || (bindings.pytest && name.id == "pytest")
            )
        }
        _ => false,
    }
}

fn invalid_metadata_diagnostics(
    fixtures: &[ParsedFixture],
    path: &Utf8PathBuf,
) -> Vec<SourceDiagnostic> {
    fixtures
        .iter()
        .flat_map(|fixture| {
            fixture.invalid.iter().map(|invalid| SourceDiagnostic {
                code: DiagnosticCode::InvalidFixture,
                message: invalid.message.clone(),
                location: SourceLocation {
                    path: path.clone(),
                    range: invalid.range,
                },
                related: Vec::new(),
            })
        })
        .collect()
}

fn duplicate_fixtures(
    fixtures: &[ParsedFixture],
    path: &Utf8PathBuf,
    diagnostics: &mut Vec<SourceDiagnostic>,
) -> HashSet<FixtureId> {
    let mut first_by_name: HashMap<&str, &ParsedFixture> = HashMap::new();
    let mut rejected = HashSet::new();
    for fixture in fixtures
        .iter()
        .filter(|fixture| fixture.public_name_known && fixture.invalid.is_empty())
    {
        if let Some(first) = first_by_name.get(fixture.definition.name.as_str()) {
            rejected.insert(first.definition.id.clone());
            rejected.insert(fixture.definition.id.clone());
            diagnostics.push(SourceDiagnostic {
                code: DiagnosticCode::DuplicateFixture,
                message: format!(
                    "Fixture `{}` is defined more than once",
                    fixture.definition.name
                ),
                location: SourceLocation {
                    path: path.clone(),
                    range: fixture.definition.name_range,
                },
                related: vec![RelatedInformation {
                    message: "First definition is here".to_owned(),
                    location: SourceLocation {
                        path: path.clone(),
                        range: first.definition.name_range,
                    },
                }],
            });
        } else {
            first_by_name.insert(&fixture.definition.name, fixture);
        }
    }
    rejected
}

fn resolve_reference(
    providers: &[FixtureProvider],
    active: Option<&FixtureDefinition>,
    name: &str,
) -> FixtureResolution {
    for provider in providers {
        if let Some(id) = provider.by_name.get(name) {
            if active.is_none_or(|fixture| fixture.id != *id) {
                return FixtureResolution::Resolved(id.clone());
            }
        }
        if let Some(rejected) = provider.rejected.get(name) {
            return FixtureResolution::Rejected(rejected.clone());
        }
        if provider.unknown {
            return FixtureResolution::Unknown;
        }
    }

    if let Some(active) = active
        && active.name == name
    {
        return FixtureResolution::Resolved(active.id.clone());
    }

    if builtin(name).is_some() {
        FixtureResolution::Builtin
    } else {
        FixtureResolution::Missing
    }
}

fn reference_diagnostics(fixtures: &[&FixtureDefinition]) -> Vec<SourceDiagnostic> {
    fixtures
        .iter()
        .flat_map(|fixture| {
            fixture.dependencies.iter().filter_map(|reference| {
                let related = match &reference.resolution {
                    FixtureResolution::Rejected(definitions) => definitions
                        .iter()
                        .map(|definition| RelatedInformation {
                            message: "Rejected fixture definition is here".to_owned(),
                            location: SourceLocation {
                                path: definition.path.clone(),
                                range: definition.range,
                            },
                        })
                        .collect(),
                    FixtureResolution::Missing => Vec::new(),
                    _ => return None,
                };
                Some(SourceDiagnostic {
                    code: DiagnosticCode::MissingFixture,
                    message: format!(
                        "Fixture `{}` requires missing fixture `{}`",
                        fixture.name, reference.name
                    ),
                    location: SourceLocation {
                        path: fixture.id.path.clone(),
                        range: reference.range,
                    },
                    related,
                })
            })
        })
        .collect()
}

fn scope_diagnostics(fixtures: &[&FixtureDefinition]) -> Vec<SourceDiagnostic> {
    let by_id = fixtures
        .iter()
        .map(|fixture| (&fixture.id, *fixture))
        .collect::<HashMap<_, _>>();
    let mut diagnostics = Vec::new();
    for fixture in fixtures {
        let Some(scope) = fixture.scope else {
            continue;
        };
        for reference in &fixture.dependencies {
            let dependency = match &reference.resolution {
                FixtureResolution::Resolved(id) => by_id.get(id).and_then(|fixture| {
                    fixture
                        .scope
                        .map(|scope| (fixture.name.as_str(), scope, Some(*fixture)))
                }),
                FixtureResolution::Builtin => {
                    builtin(&reference.name).map(|fixture| (fixture.name, fixture.scope, None))
                }
                _ => None,
            };
            let Some((dependency_name, dependency_scope, dependency)) = dependency else {
                continue;
            };
            if !scope.can_use(dependency_scope) {
                let mut related = vec![RelatedInformation {
                    message: format!("Fixture `{}` has `{}` scope", fixture.name, scope.as_str()),
                    location: SourceLocation {
                        path: fixture.id.path.clone(),
                        range: fixture.name_range,
                    },
                }];
                if let Some(dependency) = dependency {
                    related.push(RelatedInformation {
                        message: format!(
                            "Fixture `{dependency_name}` has `{}` scope",
                            dependency_scope.as_str()
                        ),
                        location: SourceLocation {
                            path: dependency.id.path.clone(),
                            range: dependency.name_range,
                        },
                    });
                }
                diagnostics.push(SourceDiagnostic {
                    code: DiagnosticCode::FixtureScopeMismatch,
                    message: format!(
                        "Fixture `{}` with `{}` scope cannot depend on fixture `{dependency_name}` with `{}` scope",
                        fixture.name,
                        scope.as_str(),
                        dependency_scope.as_str(),
                    ),
                    location: SourceLocation {
                        path: fixture.id.path.clone(),
                        range: reference.range,
                    },
                    related,
                });
            }
        }
    }
    diagnostics
}

fn cycle_diagnostics(fixtures: &[&FixtureDefinition]) -> Vec<SourceDiagnostic> {
    let by_id = fixtures
        .iter()
        .map(|fixture| (fixture.id.clone(), *fixture))
        .collect::<HashMap<_, _>>();
    let mut visited = HashSet::new();
    let mut active = Vec::new();
    let mut reported = HashSet::new();
    let mut diagnostics = Vec::new();

    for fixture in fixtures {
        visit_fixture(
            fixture,
            &by_id,
            &mut visited,
            &mut active,
            &mut reported,
            &mut diagnostics,
        );
    }
    diagnostics
}

fn visit_fixture<'a>(
    fixture: &'a FixtureDefinition,
    by_id: &HashMap<FixtureId, &'a FixtureDefinition>,
    visited: &mut HashSet<FixtureId>,
    active: &mut Vec<&'a FixtureDefinition>,
    reported: &mut HashSet<FixtureId>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if visited.contains(&fixture.id) {
        return;
    }
    active.push(fixture);
    for reference in &fixture.dependencies {
        let FixtureResolution::Resolved(id) = &reference.resolution else {
            continue;
        };
        let Some(dependency) = by_id.get(id).copied() else {
            continue;
        };
        if let Some(start) = active.iter().position(|active| active.id == dependency.id) {
            if reported.insert(dependency.id.clone()) {
                let cycle = active[start..]
                    .iter()
                    .map(|fixture| fixture.name.as_str())
                    .chain(std::iter::once(dependency.name.as_str()))
                    .collect::<Vec<_>>();
                diagnostics.push(SourceDiagnostic {
                    code: DiagnosticCode::FixtureCycle,
                    message: format!("Fixture dependency cycle: {}", cycle.join(" -> ")),
                    location: SourceLocation {
                        path: fixture.id.path.clone(),
                        range: reference.range,
                    },
                    related: active[start..]
                        .iter()
                        .map(|fixture| RelatedInformation {
                            message: format!(
                                "Fixture `{}` participates in this cycle",
                                fixture.name
                            ),
                            location: SourceLocation {
                                path: fixture.id.path.clone(),
                                range: fixture.name_range,
                            },
                        })
                        .collect(),
                });
            }
            continue;
        }
        visit_fixture(dependency, by_id, visited, active, reported, diagnostics);
    }
    let _ = active.pop();
    visited.insert(fixture.id.clone());
}

fn test_diagnostics(
    module: &CollectedModule,
    providers: &[FixtureProvider],
) -> Vec<SourceDiagnostic> {
    let path = module.path.path();
    let mut diagnostics = Vec::new();
    for test in &module.test_function_defs {
        let Some(parametrized) = parametrized_names(module, test) else {
            continue;
        };
        diagnostics.extend(
            test.parameters
                .iter_non_variadic_params()
                .filter(|parameter| !parametrized.contains(parameter.parameter.name.as_str()))
                .map(|parameter| FixtureReference {
                    name: parameter.parameter.name.to_string(),
                    range: parameter.parameter.name.range,
                    resolution: FixtureResolution::Unknown,
                })
                .filter_map(|reference| {
                    let resolution = resolve_reference(providers, None, &reference.name);
                    let related = match resolution {
                        FixtureResolution::Rejected(definitions) => definitions
                            .into_iter()
                            .map(|definition| RelatedInformation {
                                message: "Rejected fixture definition is here".to_owned(),
                                location: SourceLocation {
                                    path: definition.path,
                                    range: definition.range,
                                },
                            })
                            .collect(),
                        FixtureResolution::Missing => Vec::new(),
                        _ => return None,
                    };
                    Some(SourceDiagnostic {
                        code: DiagnosticCode::MissingFixture,
                        message: format!(
                            "Test `{}` requires missing fixture `{}`",
                            test.name, reference.name
                        ),
                        location: SourceLocation {
                            path: path.clone(),
                            range: reference.range,
                        },
                        related,
                    })
                }),
        );
    }
    diagnostics
}

pub(super) fn test_parameter_is_fixture(
    module: &CollectedModule,
    function: &StmtFunctionDef,
    name: &str,
) -> bool {
    let bindings = FixtureBindings::from_statements(&module.module_body);
    for decorator in &function.decorator_list {
        let Expr::Call(call) = &decorator.expression else {
            continue;
        };
        if !bindings.is_parametrize_reference(call.func.as_ref()) {
            continue;
        }
        let Some(argnames) = call.arguments.args.first().or_else(|| {
            call.arguments
                .keywords
                .iter()
                .find(|keyword| {
                    keyword
                        .arg
                        .as_ref()
                        .is_some_and(|argument| argument == "argnames")
                })
                .map(|keyword| &keyword.value)
        }) else {
            return false;
        };
        let Some(names) = literal_names(argnames) else {
            return false;
        };
        if names.iter().any(|parameter| parameter == name) {
            return false;
        }
    }
    true
}

/// Returns whether recognized parametrization may capture unknown parameter names.
pub(super) fn test_parametrization_is_dynamic(
    module: &CollectedModule,
    function: &StmtFunctionDef,
) -> bool {
    let bindings = FixtureBindings::from_statements(&module.module_body);
    function.decorator_list.iter().any(|decorator| {
        let Expr::Call(call) = &decorator.expression else {
            return false;
        };
        if !bindings.is_parametrize_reference(call.func.as_ref()) {
            return false;
        }
        let argnames = call.arguments.args.first().or_else(|| {
            call.arguments
                .keywords
                .iter()
                .find(|keyword| {
                    keyword
                        .arg
                        .as_ref()
                        .is_some_and(|argument| argument == "argnames")
                })
                .map(|keyword| &keyword.value)
        });
        argnames.is_none_or(|argnames| literal_names(argnames).is_none())
    })
}

/// Returns `None` when a decorator may supply arguments dynamically.
fn parametrized_names(
    module: &CollectedModule,
    function: &StmtFunctionDef,
) -> Option<HashSet<String>> {
    let bindings = FixtureBindings::from_statements(&module.module_body);
    let mut names = HashSet::new();
    for decorator in &function.decorator_list {
        let Expr::Call(call) = &decorator.expression else {
            return None;
        };
        if !bindings.is_parametrize_reference(call.func.as_ref()) {
            return None;
        }
        let argnames = call.arguments.args.first().or_else(|| {
            call.arguments
                .keywords
                .iter()
                .find(|keyword| keyword.arg.as_ref().is_some_and(|name| name == "argnames"))
                .map(|keyword| &keyword.value)
        })?;
        names.extend(literal_names(argnames)?);
    }
    Some(names)
}

fn literal_names(expression: &Expr) -> Option<Vec<String>> {
    match expression {
        Expr::StringLiteral(value) => Some(
            value
                .value
                .to_str()
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect(),
        ),
        Expr::List(list) => list.elts.iter().map(literal_name).collect(),
        Expr::Tuple(tuple) => tuple.elts.iter().map(literal_name).collect(),
        _ => None,
    }
}

fn literal_name(expression: &Expr) -> Option<String> {
    let Expr::StringLiteral(value) = expression else {
        return None;
    };
    Some(value.value.to_str().to_owned())
}

fn has_external_imports(statements: &[Stmt]) -> bool {
    statements.iter().any(|statement| match statement {
        Stmt::Import(import) => import.names.iter().any(|name| {
            !matches!(
                name.name.as_str(),
                "karva" | "pytest" | "typing" | "collections"
            )
        }),
        Stmt::ImportFrom(import) => import.module.as_ref().is_some_and(|module| {
            !matches!(
                module.as_str(),
                "karva" | "pytest" | "typing" | "collections.abc"
            )
        }),
        _ => false,
    })
}

fn builtin(name: &str) -> Option<BuiltinFixture> {
    BUILTIN_FIXTURES
        .iter()
        .find(|fixture| fixture.name == name)
        .copied()
}

#[cfg(test)]
mod tests {
    use camino::{Utf8Path, Utf8PathBuf};
    use ruff_python_ast::PythonVersion;

    use super::FixtureResolution;
    use crate::{DiagnosticCode, SourceAnalysisSettings, analyze_source, analyze_sources};

    fn analyze(source: &str) -> crate::SourceAnalysis {
        analyze_source(
            &Utf8PathBuf::from("/project/test_example.py"),
            Utf8Path::new("/project"),
            source.to_owned(),
            &SourceAnalysisSettings {
                python_version: PythonVersion::PY312,
                test_function_prefix: "test".to_owned(),
                try_import_fixtures: false,
            },
        )
        .expect("source should analyze")
    }

    fn diagnostic_codes(source: &str) -> Vec<DiagnosticCode> {
        analyze(source)
            .diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    fn module(path: &str, source: &str) -> karva_collector::CollectedModule {
        analyze_source(
            &Utf8PathBuf::from(path),
            Utf8Path::new("/project"),
            source.to_owned(),
            &SourceAnalysisSettings {
                python_version: PythonVersion::PY312,
                test_function_prefix: "test".to_owned(),
                try_import_fixtures: false,
            },
        )
        .expect("source should analyze")
        .module
    }

    #[test]
    fn reports_missing_test_fixture() {
        assert_eq!(
            diagnostic_codes("def test_example(database): pass\n"),
            [DiagnosticCode::MissingFixture]
        );
    }

    #[test]
    fn resolves_local_and_builtin_fixtures() {
        assert!(
            analyze(
                "from karva import fixture\n\n@fixture\ndef database(): pass\n\ndef test_example(database, tmp_path): pass\n"
            )
            .diagnostics
            .is_empty()
        );
    }

    #[test]
    fn reports_duplicate_fixture_names() {
        let analysis = analyze(
            "from karva import fixture\n\n@fixture\ndef first(): pass\n\n@fixture(name=\"first\")\ndef second(): pass\n",
        );

        assert_eq!(
            analysis
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code)
                .collect::<Vec<_>>(),
            [DiagnosticCode::DuplicateFixture]
        );
        assert_eq!(analysis.diagnostics[0].related.len(), 1);
    }

    #[test]
    fn reports_invalid_literal_scope() {
        let analysis = analyze(
            "from karva import fixture\n\n@fixture(scope=\"forever\")\ndef database(): pass\n",
        );

        assert_eq!(analysis.diagnostics[0].code, DiagnosticCode::InvalidFixture);
        assert_eq!(
            analysis.diagnostics[0].message,
            "Invalid fixture scope `forever`"
        );
    }

    #[test]
    fn dynamic_scope_remains_unknown() {
        assert!(
            analyze(
                "from karva import fixture\n\n@fixture(scope=choose_scope)\ndef database(missing): pass\n"
            )
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != DiagnosticCode::FixtureScopeMismatch)
        );
    }

    #[test]
    fn reports_dependency_cycle() {
        assert!(diagnostic_codes(
            "from karva import fixture\n\n@fixture\ndef first(second): pass\n\n@fixture\ndef second(first): pass\n"
        )
        .contains(&DiagnosticCode::FixtureCycle));
    }

    #[test]
    fn reports_scope_mismatch() {
        assert!(diagnostic_codes(
            "from karva import fixture\n\n@fixture\ndef narrow(): pass\n\n@fixture(scope=\"session\")\ndef broad(narrow): pass\n"
        )
        .contains(&DiagnosticCode::FixtureScopeMismatch));
    }

    #[test]
    fn parametrized_arguments_are_not_fixtures() {
        assert!(
            analyze(
                "import pytest\n\n@pytest.mark.parametrize(\"value\", [1])\ndef test_example(value, tmp_path): pass\n"
            )
            .diagnostics
            .is_empty()
        );
    }

    #[test]
    fn unresolved_imports_suppress_conftest_missing_error() {
        let analysis = analyze_source(
            &Utf8PathBuf::from("/project/conftest.py"),
            Utf8Path::new("/project"),
            "from support import external\nfrom karva import fixture\n\n@fixture\ndef local(external): pass\n".to_owned(),
            &SourceAnalysisSettings {
                python_version: PythonVersion::PY312,
                test_function_prefix: "test".to_owned(),
                try_import_fixtures: false,
            },
        )
        .expect("source should analyze");

        assert!(analysis.diagnostics.is_empty());
    }

    #[test]
    fn same_name_fixture_uses_nearest_parent_when_overridden() {
        let parent = module(
            "/project/conftest.py",
            "from karva import fixture\n\n@fixture\ndef database(): pass\n",
        );
        let current = module(
            "/project/test_example.py",
            "from karva import fixture\n\n@fixture\ndef database(database): pass\n\ndef test_example(database): pass\n",
        );
        let analysis = analyze_sources(current, &[parent], &settings());

        assert!(analysis.diagnostics.is_empty());
        let dependency = &analysis.fixtures[0].dependencies[0].resolution;
        assert!(
            matches!(dependency, FixtureResolution::Resolved(id) if id.path == "/project/conftest.py")
        );
    }

    #[test]
    fn same_name_fixture_uses_nearest_parent_across_multiple_levels() {
        let root = module(
            "/project/conftest.py",
            "from karva import fixture\n\n@fixture\ndef database(): pass\n",
        );
        let nearest = module(
            "/project/pkg/conftest.py",
            "from karva import fixture\n\n@fixture\ndef database(): pass\n",
        );
        let current = module(
            "/project/pkg/test_example.py",
            "from karva import fixture\n\n@fixture\ndef database(database): pass\n\ndef test_example(database): pass\n",
        );
        let analysis = analyze_sources(current, &[root, nearest], &settings());

        assert!(analysis.diagnostics.is_empty());
        let dependency = &analysis.fixtures[0].dependencies[0].resolution;
        assert!(
            matches!(dependency, FixtureResolution::Resolved(id) if id.path == "/project/pkg/conftest.py")
        );
    }

    #[test]
    fn cross_file_scope_mismatch_has_related_parent_location() {
        let parent = module(
            "/project/conftest.py",
            "from karva import fixture\n\n@fixture\ndef database(): pass\n",
        );
        let current = module(
            "/project/test_example.py",
            "from karva import fixture\n\n@fixture(scope=\"session\")\ndef shared(database): pass\n",
        );
        let analysis = analyze_sources(current, &[parent], &settings());

        let diagnostic = analysis
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == DiagnosticCode::FixtureScopeMismatch)
            .expect("scope mismatch");
        assert!(
            diagnostic
                .related
                .iter()
                .any(|related| related.location.path == "/project/conftest.py")
        );
        assert_eq!(diagnostic.location.path, "/project/test_example.py");
    }

    #[test]
    fn cross_file_cycle_reports_both_source_paths() {
        let parent = module(
            "/project/conftest.py",
            "from karva import fixture\n\n@fixture\ndef parent(child): pass\n",
        );
        let current = module(
            "/project/test_example.py",
            "from karva import fixture\n\n@fixture\ndef child(parent): pass\n",
        );
        let analysis = analyze_sources(current, &[parent], &settings());

        let diagnostic = analysis
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == DiagnosticCode::FixtureCycle)
            .expect("fixture cycle");
        assert!(matches!(
            diagnostic.location.path.as_str(),
            "/project/conftest.py" | "/project/test_example.py"
        ));
        assert!(
            diagnostic
                .related
                .iter()
                .any(|related| related.location.path == "/project/test_example.py")
        );
    }

    #[test]
    fn rejected_fixture_blocks_parent_fallback() {
        let parent = module(
            "/project/conftest.py",
            "from karva import fixture\n\n@fixture\ndef database(): pass\n",
        );
        let current = module(
            "/project/test_example.py",
            "from karva import fixture\n\n@fixture(scope=\"invalid\")\ndef database(): pass\n\ndef test_example(database): pass\n",
        );
        let analysis = analyze_sources(current, &[parent], &settings());

        let diagnostic = analysis
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == DiagnosticCode::MissingFixture)
            .expect("rejected fixture diagnostic");
        assert!(
            diagnostic
                .related
                .iter()
                .any(|related| related.location.path == "/project/test_example.py")
        );
    }

    #[test]
    fn unknown_fixture_decorator_suppresses_false_missing_error() {
        let analysis = analyze(
            "from custom import fixture\n\n@fixture\ndef database(): pass\n\ndef test_example(database): pass\n",
        );

        assert!(analysis.fixtures.is_empty());
        assert!(analysis.diagnostics.is_empty());
    }

    #[test]
    fn known_fixture_import_produces_a_provider() {
        let analysis = analyze(
            "from karva import fixture\n\n@fixture\ndef database(): pass\n\ndef test_example(database): pass\n",
        );

        assert_eq!(analysis.fixtures[0].name, "database");
        assert!(analysis.diagnostics.is_empty());
    }

    #[test]
    fn reports_unsupported_fixture_argument() {
        assert_eq!(
            diagnostic_codes(
                "from karva import fixture\n\n@fixture(reuse=True)\ndef database(): pass\n"
            ),
            [DiagnosticCode::InvalidFixture]
        );
    }

    #[test]
    fn karva_parametrized_arguments_are_not_fixtures() {
        assert!(
            analyze(
                "import karva\n\n@karva.tags.parametrize(\"value\", [1])\ndef test_example(value, tmp_path): pass\n"
            )
            .diagnostics
            .is_empty()
        );
    }

    #[test]
    fn unknown_parametrize_decorator_suppresses_false_missing_error() {
        assert!(diagnostic_codes(
            "import custom\n\n@custom.parametrize(\"value\", [1])\ndef test_example(value): pass\n"
        )
        .is_empty());
    }

    fn settings() -> SourceAnalysisSettings {
        SourceAnalysisSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test".to_owned(),
            try_import_fixtures: false,
        }
    }
}
