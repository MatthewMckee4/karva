//! Conservative suite checks over complete source, including uncollected definitions.

use std::collections::HashMap;

use karva_collector::CollectedModule;
use karva_python_semantic::{DecoratorBindings, KnownBinding, is_fixture_function_with_bindings};
use ruff_python_ast::comparable::ComparableExpr;
use ruff_python_ast::visitor::source_order::{self, SourceOrderVisitor};
use ruff_python_ast::{Expr, Stmt, StmtFunctionDef, UnaryOp};
use ruff_text_size::{Ranged, TextRange, TextSize};

use crate::{DiagnosticCode, RelatedInformation, SourceDiagnostic, SourceLocation};

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
