//! Conservative suite checks over complete source, including uncollected definitions.

use std::collections::{HashMap, HashSet};

use karva_collector::{CollectedModule, ModuleType};
use karva_python_semantic::{DecoratorBindings, KnownBinding, is_fixture_function_with_bindings};
use ruff_python_ast::comparable::ComparableExpr;
use ruff_python_ast::visitor::source_order::{self, SourceOrderVisitor};
use ruff_python_ast::{Expr, ExprContext, Stmt, StmtFunctionDef, UnaryOp};
use ruff_text_size::{Ranged, TextRange, TextSize};

use crate::{
    DiagnosticCode, FixtureId, FixtureOccurrenceKind, FixtureResolution, RelatedInformation,
    SourceDiagnostic, SourceLocation, WorkspaceSourceIndex, fixture_occurrences,
};

/// Detects definitions lost before runtime collection, without executing Python.
///
/// Mutually exclusive arms cannot shadow each other. Other conditional writes
/// produce an advisory rather than claiming that a definition definitely wins.
pub fn shadowed_definitions(module: &CollectedModule, test_prefix: &str) -> Vec<SourceDiagnostic> {
    let mut visitor = ShadowVisitor {
        module,
        test_prefix,
        bindings: DecoratorBindings::default(),
        seen: HashMap::new(),
        branches: Vec::new(),
        uncertain: false,
        diagnostics: Vec::new(),
    };
    visitor.visit_body(&module.module_body);
    visitor.diagnostics
}

struct Definition {
    range: TextRange,
    relevant: bool,
    branches: Vec<(TextSize, usize)>,
    uncertain: bool,
}

struct ShadowVisitor<'a> {
    module: &'a CollectedModule,
    test_prefix: &'a str,
    bindings: DecoratorBindings,
    seen: HashMap<String, Vec<Definition>>,
    branches: Vec<(TextSize, usize)>,
    uncertain: bool,
    diagnostics: Vec<SourceDiagnostic>,
}

impl ShadowVisitor<'_> {
    fn branch(&mut self, body: &[Stmt], group: TextSize, arm: usize) {
        let bindings = self.bindings.clone();
        self.branches.push((group, arm));
        self.visit_body(body);
        self.branches.pop();
        self.bindings = bindings;
    }

    fn location(&self, range: TextRange) -> SourceLocation {
        SourceLocation {
            path: self.module.path.path().clone(),
            range,
        }
    }
}

impl<'a> SourceOrderVisitor<'a> for ShadowVisitor<'_> {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::FunctionDef(function) => {
                let relevant = function.name.starts_with(self.test_prefix)
                    || is_fixture_function_with_bindings(function, &self.bindings);
                for previous in self.seen.get(function.name.as_str()).into_iter().flatten() {
                    if !(relevant || previous.relevant)
                        || previous.branches.iter().any(|(group, arm)| {
                            self.branches.iter().any(|(other_group, other_arm)| {
                                group == other_group && arm != other_arm
                            })
                        })
                    {
                        continue;
                    }
                    let uncertain = self.uncertain
                        || previous.uncertain
                        || !self.branches.is_empty()
                        || !previous.branches.is_empty();
                    let diagnostic = SourceDiagnostic {
                        code: if uncertain {
                            DiagnosticCode::PossiblyShadowedDefinition
                        } else {
                            DiagnosticCode::ShadowedDefinition
                        },
                        message: if uncertain {
                            format!(
                                "Definition `{}` may replace the earlier definition if both statements execute",
                                function.name
                            )
                        } else {
                            format!(
                                "Later definition `{}` replaces the earlier test or fixture",
                                function.name
                            )
                        },
                        location: self.location(previous.range),
                        related: vec![RelatedInformation {
                            message: if uncertain {
                                "Potential surviving definition"
                            } else {
                                "Surviving definition"
                            }
                            .to_owned(),
                            location: self.location(function.name.range()),
                        }],
                    };
                    if let Some(existing) = self.diagnostics.iter_mut().find(|existing| {
                        existing.code == diagnostic.code && existing.location == diagnostic.location
                    }) {
                        *existing = diagnostic;
                    } else {
                        self.diagnostics.push(diagnostic);
                    }
                }
                self.seen
                    .entry(function.name.to_string())
                    .or_default()
                    .push(Definition {
                        range: function.name.range(),
                        relevant,
                        branches: self.branches.clone(),
                        uncertain: self.uncertain,
                    });
                // A function's body is a separate runtime scope, not module definitions.
            }
            Stmt::ClassDef(class) => {
                let seen = std::mem::take(&mut self.seen);
                let bindings = self.bindings.clone();
                self.visit_body(&class.body);
                self.seen = seen;
                self.bindings = bindings;
            }
            Stmt::Match(statement) => {
                for (arm, case) in statement.cases.iter().enumerate() {
                    self.branch(&case.body, statement.range.start(), arm);
                }
            }
            Stmt::If(branch) => {
                let mut unresolved = false;
                let arms = std::iter::once((Some(branch.test.as_ref()), branch.body.as_slice()))
                    .chain(
                        branch
                            .elif_else_clauses
                            .iter()
                            .map(|arm| (arm.test.as_ref(), arm.body.as_slice())),
                    );
                for (index, (test, body)) in arms.enumerate() {
                    match test {
                        Some(Expr::BooleanLiteral(value)) if !value.value => {}
                        Some(Expr::BooleanLiteral(_)) | None => {
                            if unresolved {
                                self.branch(body, branch.range.start(), index);
                            } else {
                                self.visit_body(body);
                            }
                            break;
                        }
                        Some(_) => {
                            unresolved = true;
                            self.branch(body, branch.range.start(), index);
                        }
                    }
                }
            }
            _ => {
                let uncertain = self.uncertain;
                self.uncertain = true;
                source_order::walk_stmt(self, statement);
                self.uncertain = uncertain;
            }
        }
        self.bindings.update(statement);
    }
}

/// Compares literal cases without importing modules, calling constructors, or
/// invoking user-defined equality. Unknown expressions never establish equality.
pub fn duplicate_parameter_cases(module: &CollectedModule) -> Vec<SourceDiagnostic> {
    let mut diagnostics = Vec::new();
    for function in &module.test_function_defs {
        let bindings = DecoratorBindings::before(&module.module_body, function.range.start());
        parameter_diagnostics(module, function, &bindings, &mut diagnostics);
    }
    diagnostics
}

fn parameter_diagnostics(
    module: &CollectedModule,
    function: &StmtFunctionDef,
    bindings: &DecoratorBindings,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    for decorator in &function.decorator_list {
        let Expr::Call(call) = &decorator.expression else {
            continue;
        };
        if !bindings.is_parametrize(&call.func) {
            continue;
        }
        if call
            .arguments
            .args
            .iter()
            .any(|argument| matches!(argument, Expr::Starred(_)))
            || call
                .arguments
                .keywords
                .iter()
                .any(|keyword| keyword.arg.is_none())
        {
            continue;
        }
        let values = call
            .arguments
            .args
            .get(1)
            .or_else(|| keyword(call, "argvalues"));
        let Some(values) = values.and_then(sequence) else {
            continue;
        };
        let ids = keyword(call, "ids").and_then(sequence);
        let mut seen_values = HashMap::new();
        let mut seen_ids = HashMap::new();
        for (index, value) in values.iter().enumerate() {
            let explicit = explicit_case(value, bindings);
            let arguments = if let Some(case) = explicit {
                // Marks can intentionally distinguish cases with the same input.
                if case
                    .arguments
                    .keywords
                    .iter()
                    .any(|kw| kw.arg.as_ref().is_none_or(|name| name.as_str() != "id"))
                {
                    None
                } else {
                    Some(case.arguments.args.as_ref())
                }
            } else {
                Some(std::slice::from_ref(value))
            };
            if let Some(arguments) = arguments.filter(|arguments| arguments.iter().all(literal)) {
                let key = arguments
                    .iter()
                    .map(ComparableExpr::from)
                    .collect::<Vec<_>>();
                if let Some((first, range)) = seen_values.get(&key) {
                    diagnostics.push(duplicate_case(
                        module,
                        DiagnosticCode::DuplicateParameterCase,
                        index,
                        *first,
                        value.range(),
                        *range,
                    ));
                } else {
                    seen_values.insert(key, (index, value.range()));
                }
            }
            let id = match explicit {
                Some(case)
                    if case
                        .arguments
                        .keywords
                        .iter()
                        .any(|keyword| keyword.arg.is_none()) =>
                {
                    None
                }
                Some(case) => match keyword(case, "id") {
                    Some(id @ Expr::StringLiteral(_)) => Some(id),
                    Some(Expr::NoneLiteral(_)) | None => ids.and_then(|ids| ids.get(index)),
                    Some(_) => None,
                },
                None if literal(value) => ids.and_then(|ids| ids.get(index)),
                None => None,
            };
            if let Some(Expr::StringLiteral(id)) = id {
                let key = id.value.to_str();
                if let Some((first, range)) = seen_ids.get(key) {
                    diagnostics.push(duplicate_case(
                        module,
                        DiagnosticCode::DuplicateParameterId,
                        index,
                        *first,
                        id.range,
                        *range,
                    ));
                } else {
                    seen_ids.insert(key, (index, id.range));
                }
            }
        }
    }
}

fn duplicate_case(
    module: &CollectedModule,
    code: DiagnosticCode,
    index: usize,
    first: usize,
    range: TextRange,
    first_range: TextRange,
) -> SourceDiagnostic {
    SourceDiagnostic {
        code,
        message: format!(
            "Parameter case {} repeats {} from case {}",
            index + 1,
            if code == DiagnosticCode::DuplicateParameterId {
                "the explicit ID"
            } else {
                "literal arguments"
            },
            first + 1
        ),
        location: SourceLocation {
            path: module.path.path().clone(),
            range,
        },
        related: vec![RelatedInformation {
            message: format!("First occurrence: case {}", first + 1),
            location: SourceLocation {
                path: module.path.path().clone(),
                range: first_range,
            },
        }],
    }
}

fn keyword<'a>(call: &'a ruff_python_ast::ExprCall, name: &str) -> Option<&'a Expr> {
    call.arguments
        .keywords
        .iter()
        .find(|kw| kw.arg.as_ref().is_some_and(|arg| arg.as_str() == name))
        .map(|kw| &kw.value)
}

/// Unpacking makes subsequent case indices unknown, so no indexed comparison is safe.
fn sequence(expression: &Expr) -> Option<&[Expr]> {
    let elements = match expression {
        Expr::List(list) => &list.elts,
        Expr::Tuple(tuple) => &tuple.elts,
        _ => return None,
    };
    (!elements
        .iter()
        .any(|element| matches!(element, Expr::Starred(_))))
    .then_some(elements)
}

fn explicit_case<'a>(
    expression: &'a Expr,
    bindings: &DecoratorBindings,
) -> Option<&'a ruff_python_ast::ExprCall> {
    let Expr::Call(call) = expression else {
        return None;
    };
    let recognized = match call.func.as_ref() {
        Expr::Name(name) => bindings.get(name.id.as_str()) == Some(KnownBinding::Param),
        Expr::Attribute(attr) => {
            attr.attr.as_str() == "param"
                && matches!(
                    bindings.binding_for(&attr.value),
                    Some(KnownBinding::Karva | KnownBinding::Pytest)
                )
        }
        _ => false,
    };
    recognized.then_some(call)
}

fn literal(expression: &Expr) -> bool {
    match expression {
        Expr::StringLiteral(_)
        | Expr::BytesLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BooleanLiteral(_)
        | Expr::NoneLiteral(_)
        | Expr::EllipsisLiteral(_) => true,
        Expr::UnaryOp(unary) => {
            matches!(unary.op, UnaryOp::UAdd | UnaryOp::USub)
                && matches!(unary.operand.as_ref(), Expr::NumberLiteral(_))
        }
        Expr::List(list) => list.elts.iter().all(literal),
        Expr::Tuple(tuple) => tuple.elts.iter().all(literal),
        Expr::Dict(dict) => dict
            .items
            .iter()
            .all(|item| item.key.as_ref().is_some_and(literal) && literal(&item.value)),
        Expr::Set(set) => set.elts.iter().all(literal),
        _ => false,
    }
}

/// Reports unreachable local fixtures only when source lookup is complete.
///
/// Callers must index the full configured suite. Dynamic lookup or unknown
/// provider metadata prevents a closed graph and therefore suppresses this rule.
pub fn unused_fixtures(index: &WorkspaceSourceIndex) -> Vec<SourceDiagnostic> {
    let mut candidates = HashMap::new();
    let mut edges: HashMap<FixtureId, HashSet<FixtureId>> = HashMap::new();
    let mut live = HashSet::new();
    for path in index.paths() {
        let Some(analysis) = index.analyze(path) else {
            continue;
        };
        if analysis.module.path.module_name() == "karva._builtins" {
            continue;
        }
        let model = &analysis.fixture_model;
        let mut all_names = HealthNames::default();
        all_names.visit_body(&analysis.module.module_body);
        if all_names.dynamic
            || !model.builtins_visible()
            || analysis
                .module
                .test_function_defs
                .iter()
                .any(|function| model.parametrization_is_dynamic(function))
        {
            return Vec::new();
        }
        // A fixture-only module may be a public provider imported outside this suite.
        let public_api = analysis.module.module_type() == ModuleType::Test
            && analysis.module.test_function_defs.is_empty();
        let exported = all_names.names.contains("__all__") || public_api;
        for definition in model.local() {
            candidates.insert(definition.id.clone(), definition.name.clone());
            if exported || definition.auto_use != Some(false) {
                live.insert(definition.id.clone());
            }
        }
        for definition in model.visible().iter().chain(model.local()) {
            if definition.auto_use != Some(false) {
                live.insert(definition.id.clone());
            }
            for dependency in &definition.dependencies {
                match &dependency.resolution {
                    FixtureResolution::Unknown => return Vec::new(),
                    FixtureResolution::Resolved(id) => {
                        edges
                            .entry(definition.id.clone())
                            .or_default()
                            .insert(id.clone());
                    }
                    _ => {}
                }
            }
        }
        for occurrence in fixture_occurrences(&analysis) {
            if matches!(
                occurrence.kind,
                FixtureOccurrenceKind::TestParameter
                    | FixtureOccurrenceKind::UseFixtures
                    | FixtureOccurrenceKind::Import
            ) {
                live.insert(occurrence.fixture);
            }
        }
        // Ordinary Python uses can expose a fixture API independently of injection.
        let mut roots = HealthNames {
            skip: analysis
                .module
                .fixture_function_defs
                .iter()
                .map(Ranged::range)
                .collect(),
            ..HealthNames::default()
        };
        roots.visit_body(&analysis.module.module_body);
        for name in roots.names {
            if let Some(definition) = model.resolve(&name) {
                live.insert(definition.id.clone());
            }
        }
        for definition in model.local() {
            if let Some(function) = analysis
                .module
                .fixture_function_defs
                .iter()
                .find(|function| function.name.range == definition.name_range)
            {
                let mut names = HealthNames::default();
                names.visit_body(&function.body);
                for name in names.names {
                    if let Some(provider) = model.resolve(&name) {
                        edges
                            .entry(definition.id.clone())
                            .or_default()
                            .insert(provider.id.clone());
                    }
                }
            }
        }
    }
    let mut pending = live.iter().cloned().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        if let Some(dependencies) = edges.get(&id) {
            for dependency in dependencies {
                if live.insert(dependency.clone()) {
                    pending.push(dependency.clone());
                }
            }
        }
    }
    let mut findings = candidates
        .into_iter()
        .filter(|(id, _)| !live.contains(id))
        .map(|(id, name)| SourceDiagnostic {
            code: DiagnosticCode::UnusedFixture,
            message: format!(
                "Fixture `{name}` has no direct or transitive consumer in the full configured suite"
            ),
            location: SourceLocation {
                path: id.path,
                range: id.range,
            },
            related: Vec::new(),
        })
        .collect::<Vec<_>>();
    findings.sort_by(|a, b| {
        (&a.location.path, a.location.range.start())
            .cmp(&(&b.location.path, b.location.range.start()))
    });
    findings
}

#[derive(Default)]
struct HealthNames {
    names: HashSet<String>,
    dynamic: bool,
    skip: HashSet<TextRange>,
}

impl<'a> SourceOrderVisitor<'a> for HealthNames {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        if !self.skip.contains(&statement.range()) {
            source_order::walk_stmt(self, statement);
        }
    }

    fn visit_expr(&mut self, expression: &'a Expr) {
        match expression {
            Expr::Name(name) => {
                // Export declarations are roots even though their name is a store.
                if name.ctx == ExprContext::Load || name.id.as_str() == "__all__" {
                    self.names.insert(name.id.to_string());
                }
                self.dynamic |= matches!(
                    name.id.as_str(),
                    "getfixturevalue"
                        | "get_fixture"
                        | "globals"
                        | "locals"
                        | "eval"
                        | "exec"
                        | "getattr"
                        | "pytest_plugins"
                        | "karva_plugins"
                );
            }
            Expr::Attribute(attr) => {
                self.dynamic |= matches!(attr.attr.as_str(), "getfixturevalue" | "get_fixture");
            }
            _ => {}
        }
        source_order::walk_expr(self, expression);
    }
}
