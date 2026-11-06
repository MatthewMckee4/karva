//! Source-only suite health checks, independent of Python imports and execution.

use std::collections::BTreeMap;

use karva_collector::CollectedModule;
use karva_python_semantic::{DecoratorBindings, is_fixture_function_with_bindings};
use ruff_python_ast::{Expr, Stmt};
use ruff_text_size::{Ranged, TextRange, TextSize};

use crate::{DiagnosticCode, RelatedInformation, SourceDiagnostic, SourceLocation};

/// Reports definitions that can replace an earlier test or fixture in the same scope.
///
/// Mutually exclusive arms of one branch cannot shadow each other. Uncertain
/// control flow is reported explicitly rather than executing conditions.
pub fn source_health(module: &CollectedModule, test_prefix: &str) -> Vec<SourceDiagnostic> {
    let mut checker = Checker {
        module,
        prefix: test_prefix,
        diagnostics: Vec::new(),
    };
    checker.check_scope(&module.module_body, true, &DecoratorBindings::default());
    checker
        .diagnostics
        .sort_by_key(|diagnostic| diagnostic.location.range.start());
    checker.diagnostics
}

/// Shared reporting context; each lexical scope gets its own definition table.
struct Checker<'a> {
    module: &'a CollectedModule,
    prefix: &'a str,
    diagnostics: Vec<SourceDiagnostic>,
}

/// One binding in a lexical scope, with its control-flow provenance.
struct Definition {
    range: TextRange,
    relevant: bool,
    /// Branch statement offsets and mutually exclusive arm indices.
    branches: Vec<(TextSize, usize)>,

    /// Execution depends on a condition that was not evaluated statically.
    uncertain: bool,
}

impl Checker<'_> {
    fn check_scope(&mut self, body: &[Stmt], collect_tests: bool, bindings: &DecoratorBindings) {
        let mut definitions = BTreeMap::<String, Vec<Definition>>::new();
        self.scan(
            body,
            collect_tests,
            &mut bindings.clone(),
            &[],
            false,
            &mut definitions,
        );
        for (name, definitions) in definitions {
            for (index, earlier) in definitions
                .iter()
                .enumerate()
                .filter(|(_, definition)| definition.relevant)
            {
                let later = definitions[index + 1..].iter().rev().find(|later| {
                    !earlier.branches.iter().any(|(branch, arm)| {
                        later.branches.iter().any(|(other_branch, other_arm)| {
                            branch == other_branch && arm != other_arm
                        })
                    })
                });
                let Some(later) = later else {
                    continue;
                };
                let uncertain = earlier.uncertain || later.uncertain;
                let message = if uncertain {
                    format!(
                        "`{name}` may be shadowed by the later definition if both execute; the later definition wins. Rename or remove one definition"
                    )
                } else {
                    format!(
                        "`{name}` is shadowed by the later definition, which wins. Rename or remove one definition"
                    )
                };
                self.diagnostics.push(SourceDiagnostic {
                    code: DiagnosticCode::ShadowedDefinition,
                    message,
                    location: SourceLocation {
                        path: self.module.path.path().clone(),
                        range: earlier.range,
                    },
                    related: vec![RelatedInformation {
                        message: "Later definition replaces the earlier binding".to_owned(),
                        location: SourceLocation {
                            path: self.module.path.path().clone(),
                            range: later.range,
                        },
                    }],
                });
            }
        }
    }

    fn scan(
        &mut self,
        body: &[Stmt],
        collect_tests: bool,
        bindings: &mut DecoratorBindings,
        branches: &[(TextSize, usize)],
        uncertain: bool,
        definitions: &mut BTreeMap<String, Vec<Definition>>,
    ) {
        for statement in body {
            match statement {
                Stmt::FunctionDef(function) => {
                    let relevant = collect_tests && function.name.starts_with(self.prefix)
                        || is_fixture_function_with_bindings(function, bindings);
                    definitions
                        .entry(function.name.to_string())
                        .or_default()
                        .push(Definition {
                            range: function.name.range,
                            relevant,
                            branches: branches.to_vec(),
                            uncertain,
                        });
                    let mut local = bindings.clone();
                    for parameter in function.parameters.iter_non_variadic_params() {
                        local.shadow(parameter.parameter.name.as_str());
                    }
                    if let Some(parameter) = &function.parameters.vararg {
                        local.shadow(parameter.name.as_str());
                    }
                    if let Some(parameter) = &function.parameters.kwarg {
                        local.shadow(parameter.name.as_str());
                    }
                    self.check_scope(&function.body, false, &local);
                }
                Stmt::ClassDef(class) => self.check_scope(&class.body, true, bindings),
                Stmt::If(statement) => {
                    let mut can_fall_through = true;
                    let mut dynamic = false;
                    for (arm, (test, body)) in
                        std::iter::once((Some(statement.test.as_ref()), statement.body.as_slice()))
                            .chain(
                                statement
                                    .elif_else_clauses
                                    .iter()
                                    .map(|clause| (clause.test.as_ref(), clause.body.as_slice())),
                            )
                            .enumerate()
                    {
                        if !can_fall_through {
                            break;
                        }
                        let known = test.map_or(Some(true), |test| match test {
                            Expr::BooleanLiteral(value) => Some(value.value),
                            _ => None,
                        });
                        if known == Some(false) {
                            continue;
                        }
                        let conditional = dynamic || known.is_none();
                        let mut arm_branches = branches.to_vec();
                        if conditional {
                            arm_branches.push((statement.start(), arm));
                        }
                        let mut local = bindings.clone();
                        let branch_bindings = if conditional {
                            &mut local
                        } else {
                            &mut *bindings
                        };
                        self.scan(
                            body,
                            collect_tests,
                            branch_bindings,
                            &arm_branches,
                            uncertain || conditional,
                            definitions,
                        );
                        dynamic |= known.is_none();
                        can_fall_through = known != Some(true);
                    }
                    // A proven branch has already updated the live binding environment.
                    if !dynamic {
                        continue;
                    }
                }
                Stmt::For(statement) => {
                    self.scan(
                        &statement.body,
                        collect_tests,
                        &mut bindings.clone(),
                        branches,
                        true,
                        definitions,
                    );
                    self.scan(
                        &statement.orelse,
                        collect_tests,
                        &mut bindings.clone(),
                        branches,
                        true,
                        definitions,
                    );
                }
                Stmt::While(statement) => {
                    if !matches!(statement.test.as_ref(), Expr::BooleanLiteral(value) if !value.value)
                    {
                        self.scan(
                            &statement.body,
                            collect_tests,
                            &mut bindings.clone(),
                            branches,
                            true,
                            definitions,
                        );
                    }
                    self.scan(
                        &statement.orelse,
                        collect_tests,
                        &mut bindings.clone(),
                        branches,
                        true,
                        definitions,
                    );
                }
                Stmt::With(statement) => self.scan(
                    &statement.body,
                    collect_tests,
                    bindings,
                    branches,
                    uncertain,
                    definitions,
                ),
                Stmt::Try(statement) => {
                    for body in std::iter::once(statement.body.as_slice())
                        .chain(statement.handlers.iter().map(|handler| {
                            let ruff_python_ast::ExceptHandler::ExceptHandler(handler) = handler;
                            handler.body.as_slice()
                        }))
                        .chain([statement.orelse.as_slice(), statement.finalbody.as_slice()])
                    {
                        self.scan(
                            body,
                            collect_tests,
                            &mut bindings.clone(),
                            branches,
                            true,
                            definitions,
                        );
                    }
                }
                Stmt::Match(statement) => {
                    for (arm, case) in statement.cases.iter().enumerate() {
                        let mut arm_branches = branches.to_vec();
                        arm_branches.push((statement.start(), arm));
                        self.scan(
                            &case.body,
                            collect_tests,
                            &mut bindings.clone(),
                            &arm_branches,
                            true,
                            definitions,
                        );
                    }
                }
                _ => {}
            }
            bindings.update(statement);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use camino::Utf8PathBuf;
    use karva_collector::{CollectionSettings, collect_source};
    use ruff_python_ast::PythonVersion;

    fn check(source: &str) -> Vec<SourceDiagnostic> {
        let root = Utf8PathBuf::from("/project");
        let module = collect_source(
            &root.join("test_health.py"),
            &root,
            source.to_owned(),
            &CollectionSettings {
                python_version: PythonVersion::latest(),
                test_function_prefix: "test",
                respect_ignore_files: true,
                collect_fixtures: true,
                collect_doctests: false,
            },
            &[],
        )
        .expect("parse source");
        source_health(&module, "test")
    }

    #[rstest::rstest]
    #[case("def test_same(): pass\ndef test_same(): pass\n")]
    #[case("class TestExample:\n def test_same(self): pass\n def test_same(self): pass\n")]
    #[case("from karva import fixture\n@fixture\ndef data(): pass\ndef data(): pass\n")]
    #[case(
        "from karva import fixture\nif False:\n fixture = other\n@fixture\ndef data(): pass\ndef data(): pass\n"
    )]
    #[case(
        "if True:\n from karva import fixture as provider\n@provider\ndef data(): pass\ndef data(): pass\n"
    )]
    fn points_from_shadowed_binding_to_the_surviving_definition(#[case] source: &str) {
        let diagnostics = check(source);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::ShadowedDefinition);
        assert!(
            diagnostics[0].location.range.start()
                < diagnostics[0].related[0].location.range.start()
        );
        assert!(diagnostics[0].message.contains("which wins"));
    }

    #[rstest::rstest]
    #[case("def test_first(): pass\ndef test_second(): pass\n")]
    #[case("class TestA:\n def test_same(self): pass\nclass TestB:\n def test_same(self): pass\n")]
    #[case("if condition:\n def test_same(): pass\nelse:\n def test_same(): pass\n")]
    #[case("if False:\n def test_same(): pass\ndef test_same(): pass\n")]
    #[case("if True:\n def test_same(): pass\nelse:\n def test_same(): pass\n")]
    #[case("def helper():\n def test_same(): pass\n def test_same(): pass\n")]
    #[case(
        "from karva import fixture\ndef helper(fixture):\n @fixture\n def data(): pass\n @fixture\n def data(): pass\n"
    )]
    fn distinct_scopes_and_exclusive_or_unreachable_branches_are_clean(#[case] source: &str) {
        assert_eq!(check(source), []);
    }

    #[test]
    fn independent_dynamic_branches_report_uncertainty() {
        let diagnostics =
            check("if first:\n def test_same(): pass\nif second:\n def test_same(): pass\n");
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].message.contains("if both execute"));
    }
}
