//! Conservative local fixture stubs for definite missing-provider diagnostics.

use std::collections::HashSet;

use ruff_python_ast::visitor::source_order::{self, SourceOrderVisitor};
use ruff_python_ast::{Expr, ExprContext, Identifier, Stmt};
use ruff_text_size::{Ranged, TextRange, TextSize};

use crate::{
    DiagnosticCode, FixtureScope, SourceAnalysis, SourceDiagnostic, is_valid_fixture_name,
};

/// A local fixture declaration and import offered for a diagnostic in the current document.
#[derive(Debug)]
pub struct FixtureCodeAction {
    /// User-visible choice; never applied automatically.
    pub title: String,

    /// Diagnostic that was recomputed from the same source snapshot.
    pub diagnostic: SourceDiagnostic,

    /// UTF-8 byte position after the module header and initial imports.
    pub import_offset: TextSize,

    /// Decorator import that introduces a previously unused binding.
    pub import_text: String,

    /// UTF-8 byte range where the local declaration is inserted.
    pub range: TextRange,

    /// Complete Python source to insert.
    pub new_text: String,
}

/// Offers local stubs without guessing a provider's package or changing existing bindings.
///
/// Dynamic, rejected, and non-identifier fixture names are excluded. The generated
/// body fails explicitly until implemented. Existing code, including imports, is
/// left intact; a new decorator import follows the module header and initial imports.
pub fn fixture_code_actions(analysis: &SourceAnalysis) -> Vec<FixtureCodeAction> {
    let source = &analysis.module.source_text;
    let mut identifiers = Identifiers::default();
    source_order::walk_body(&mut identifiers, &analysis.module.module_body);
    if identifiers.bindings.contains("NotImplementedError") {
        return Vec::new();
    }
    let (import, decorator) = if !identifiers.names.contains("fixture") {
        ("from karva import fixture", "fixture")
    } else if !identifiers.names.contains("karva") {
        ("import karva", "karva.fixture")
    } else {
        return Vec::new();
    };
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let indent = analysis
        .module
        .test_function_defs
        .iter()
        .chain(&analysis.module.fixture_function_defs)
        .flat_map(|function| &function.body)
        .find_map(|statement| {
            let before = source.get(..statement.start().to_usize())?;
            let indent = before.rsplit('\n').next()?;
            (!indent.is_empty() && indent.chars().all(|c| c == ' ' || c == '\t')).then_some(indent)
        })
        .unwrap_or("    ");
    let Ok(end) = TextSize::try_from(source.len()) else {
        return Vec::new();
    };
    let mut actions = Vec::new();
    for diagnostic in &analysis.diagnostics {
        if diagnostic.code != DiagnosticCode::MissingFixture {
            continue;
        }
        let range = diagnostic.location.range;
        let Some(name) = source.get(range.start().to_usize()..range.end().to_usize()) else {
            continue;
        };
        if !is_valid_fixture_name(name) || identifiers.bindings.contains(name) {
            continue;
        }
        let scope = analysis
            .fixture_model
            .local()
            .iter()
            .filter(|fixture| {
                fixture
                    .dependencies
                    .iter()
                    .any(|dependency| dependency.name == name)
            })
            .try_fold(FixtureScope::Function, |scope, fixture| {
                fixture.scope.map(|other| scope.max(other))
            });
        let Some(scope) = scope else {
            continue;
        };
        let scope_arg = if scope == FixtureScope::Function {
            String::new()
        } else {
            format!("(scope=\"{}\")", scope.as_str())
        };
        let separator = if source.ends_with(newline) {
            newline.to_owned()
        } else {
            format!("{newline}{newline}")
        };
        actions.push(FixtureCodeAction {
            title: format!("Create local fixture `{name}`"),
            diagnostic: diagnostic.clone(),
            import_offset: import_offset(analysis),
            import_text: format!("{import}{newline}"),
            range: TextRange::empty(end),
            new_text: format!("{separator}@{decorator}{scope_arg}{newline}def {name}():{newline}{indent}raise NotImplementedError{newline}"),
        });
    }
    actions
}

/// Keeps leading comments, the module docstring, and future imports ahead of new imports.
fn import_offset(analysis: &SourceAnalysis) -> TextSize {
    let source = &analysis.module.source_text;
    let mut offset = TextSize::ZERO;
    for (index, statement) in analysis.module.module_body.iter().enumerate() {
        let header = matches!(statement, Stmt::Import(_) | Stmt::ImportFrom(_))
            || index == 0
                && matches!(statement, Stmt::Expr(expression) if matches!(&*expression.value, Expr::StringLiteral(_)));
        if !header {
            if offset == TextSize::ZERO {
                let start = match statement {
                    Stmt::FunctionDef(function) => function
                        .decorator_list
                        .first()
                        .map_or_else(|| statement.start(), Ranged::start),
                    Stmt::ClassDef(class) => class
                        .decorator_list
                        .first()
                        .map_or_else(|| statement.start(), Ranged::start),
                    _ => statement.start(),
                };
                let prefix = &source[..start.to_usize()];
                let position = prefix.rfind('\n').map_or(0, |newline| newline + 1);
                return TextSize::try_from(position).unwrap_or(TextSize::ZERO);
            }
            return offset;
        }
        let end = statement.end().to_usize();
        let position = source[end..]
            .find('\n')
            .map_or(end, |newline| end + newline + 1);
        offset = TextSize::try_from(position).unwrap_or_else(|_| statement.end());
    }
    offset
}

#[derive(Default)]
struct Identifiers {
    names: HashSet<String>,
    bindings: HashSet<String>,
}

impl<'a> SourceOrderVisitor<'a> for Identifiers {
    fn visit_identifier(&mut self, identifier: &'a Identifier) {
        self.names.insert(identifier.as_str().to_owned());
    }

    fn visit_expr(&mut self, expression: &'a Expr) {
        if let Expr::Name(name) = expression {
            self.names.insert(name.id.to_string());
            if name.ctx == ExprContext::Store {
                self.bindings.insert(name.id.to_string());
            }
        }
        source_order::walk_expr(self, expression);
    }

    fn visit_stmt(&mut self, statement: &'a Stmt) {
        match statement {
            Stmt::FunctionDef(function) => {
                self.bindings.insert(function.name.to_string());
            }
            Stmt::ClassDef(class) => {
                self.bindings.insert(class.name.to_string());
            }
            Stmt::Import(import) => {
                for alias in &import.names {
                    self.bindings.insert(
                        alias
                            .asname
                            .as_ref()
                            .map_or_else(
                                || {
                                    alias
                                        .name
                                        .as_str()
                                        .split('.')
                                        .next()
                                        .unwrap_or(alias.name.as_str())
                                },
                                Identifier::as_str,
                            )
                            .to_owned(),
                    );
                }
            }
            _ => {}
        }
        // Module paths do not bind names for a from-import.
        if let Stmt::ImportFrom(import) = statement {
            for alias in &import.names {
                self.bindings.insert(
                    alias
                        .asname
                        .as_ref()
                        .unwrap_or(&alias.name)
                        .as_str()
                        .to_owned(),
                );
                self.names.insert(
                    alias
                        .asname
                        .as_ref()
                        .unwrap_or(&alias.name)
                        .as_str()
                        .to_owned(),
                );
            }
        } else {
            source_order::walk_stmt(self, statement);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SourceAnalysisSettings, analyze_source, source_symbols};
    use camino::Utf8PathBuf;
    use ruff_python_ast::PythonVersion;

    fn analyze(source: &str) -> SourceAnalysis {
        let root = Utf8PathBuf::from("/project");
        analyze_source(
            &root.join("test_example.py"),
            &root,
            source.to_owned(),
            &SourceAnalysisSettings {
                python_version: PythonVersion::PY313,
                test_function_prefix: "test".to_owned(),
                try_import_fixtures: false,
            },
        )
        .expect("source should parse")
    }

    #[test]
    fn generated_stub_parses_and_resolves_missing_dependency() {
        let source = "from karva import fixture\n@fixture(scope='session')\ndef shared(database):\n\treturn database\n";
        let analysis = analyze(source);
        let actions = fixture_code_actions(&analysis);
        assert_eq!(actions.len(), 1);
        assert!(
            actions[0]
                .new_text
                .contains("@karva.fixture(scope=\"session\")")
        );
        assert!(actions[0].new_text.contains("\traise NotImplementedError"));
        let mut edited = format!("{source}{}", actions[0].new_text);
        edited.insert_str(actions[0].import_offset.to_usize(), &actions[0].import_text);
        let analysis = analyze(&edited);
        assert!(analysis.diagnostics.is_empty());
        assert!(
            source_symbols(&analysis)
                .iter()
                .any(|symbol| symbol.name == "database")
        );
    }

    #[test]
    fn generated_unicode_stub_preserves_crlf() {
        let source = "def test_example(данные):\r\n    pass\r\n";
        let actions = fixture_code_actions(&analyze(source));
        assert_eq!(actions.len(), 1);
        let inserted = &actions[0].new_text;
        assert!(!inserted.replace("\r\n", "").contains('\n'));
        assert!(
            analyze(&format!("{source}{inserted}"))
                .diagnostics
                .is_empty()
        );
    }

    #[test]
    fn header_imports_and_all_consumer_scopes_are_preserved() {
        let source = "#!/usr/bin/env python3\n\"\"\"Project tests.\"\"\"\nfrom __future__ import annotations\nfrom karva import fixture\n\n@fixture(scope='session')\ndef shared(database): return database\n\ndef test_example(database): pass\n";
        let actions = fixture_code_actions(&analyze(source));
        assert_eq!(actions.len(), 2);
        let action = &actions[0];
        assert!(action.new_text.contains("scope=\"session\""));
        let mut edited = format!("{source}{}", action.new_text);
        edited.insert_str(action.import_offset.to_usize(), &action.import_text);
        assert!(edited.starts_with("#!/usr/bin/env python3\n\"\"\"Project tests.\"\"\"\nfrom __future__ import annotations\nfrom karva import fixture\nimport karva\n"));
        assert!(analyze(&edited).diagnostics.is_empty());
    }

    #[test]
    fn existing_module_binding_is_not_overwritten() {
        let source = "database = 42\ndef test_example(database): pass\n";
        assert!(fixture_code_actions(&analyze(source)).is_empty());
    }
}
