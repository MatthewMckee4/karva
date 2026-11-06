//! Conservative suite checks over complete source, including uncollected definitions.

use std::collections::HashMap;

use karva_collector::CollectedModule;
use karva_python_semantic::{DecoratorBindings, is_fixture_function_with_bindings};
use ruff_python_ast::visitor::source_order::{self, SourceOrderVisitor};
use ruff_python_ast::{Expr, Stmt};
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
