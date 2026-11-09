//! AST-directed edits, token operators, and source exclusion pragmas.

use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use camino::Utf8Path;
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;
use ruff_python_ast::token::{TokenKind, Tokens};
use ruff_python_ast::visitor::{Visitor, walk_expr, walk_stmt};
use ruff_python_ast::{Expr, Number, PythonVersion, Stmt, UnaryOp};
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
use ruff_text_size::{Ranged, TextRange};

use crate::{Mutant, Operator, fingerprint};

/// Validated filters shared by every discovered file.
#[derive(Default)]
pub(crate) struct DiscoveryFilters {
    operators: BTreeSet<Operator>,
    symbols: Option<GlobSet>,
    exclude_symbols: Option<GlobSet>,
    exclude_patterns: Vec<Regex>,
}

impl DiscoveryFilters {
    /// Validates operator names, symbol globs, and line exclusion regular expressions.
    fn new(
        operators: &[String],
        symbols: &[String],
        exclude_symbols: &[String],
        patterns: &[String],
    ) -> Result<Self> {
        fn globs(patterns: &[String]) -> Result<Option<GlobSet>> {
            if patterns.is_empty() {
                return Ok(None);
            }
            let mut builder = GlobSetBuilder::new();
            for pattern in patterns {
                builder.add(
                    Glob::new(pattern)
                        .with_context(|| format!("invalid mutation symbol pattern `{pattern}`"))?,
                );
            }
            Ok(Some(builder.build()?))
        }
        Ok(Self {
            operators: operators
                .iter()
                .map(|value| Operator::parse(value))
                .collect::<Result<_>>()?,
            symbols: globs(symbols)?,
            exclude_symbols: globs(exclude_symbols)?,
            exclude_patterns: patterns
                .iter()
                .map(|pattern| {
                    Regex::new(pattern)
                        .with_context(|| format!("invalid mutation exclusion pattern `{pattern}`"))
                })
                .collect::<Result<_>>()?,
        })
    }
}

/// Enumerates parseable, independent edits in deterministic source order.
///
/// `path` is project-relative and normalized by the caller. Pragmas are comments:
/// `# pragma: no mutate` excludes that line, and `# karva: mutate off` / `on`
/// delimit excluded blocks. Text inside string literals cannot activate pragmas.
fn discover(
    path: &Utf8Path,
    source: &str,
    version: PythonVersion,
    filters: &DiscoveryFilters,
) -> Result<Vec<Mutant>> {
    let options = ParseOptions::from(Mode::Module).with_target_version(version);
    let parsed = parse_unchecked(source, options.clone());
    if parsed.has_syntax_errors() {
        bail!(
            "cannot discover mutations in `{path}`: {:?}; {:?}",
            parsed.errors(),
            parsed.unsupported_syntax_errors()
        );
    }
    let mut excluded = BTreeSet::new();
    let mut disabled = false;
    let mut pragma_lines = std::collections::BTreeMap::new();
    for token in parsed.tokens() {
        if token.kind() == TokenKind::Comment {
            let line = source[..usize::from(token.start())]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            pragma_lines.insert(line, &source[token.range()]);
        }
    }
    for (index, line) in source.lines().enumerate() {
        let number = index + 1;
        let comment = pragma_lines.get(&number).copied().unwrap_or_default();
        if comment.trim() == "# karva: mutate off" {
            disabled = true;
        }
        if disabled
            || comment.contains("pragma: no mutate")
            || filters
                .exclude_patterns
                .iter()
                .any(|pattern| pattern.is_match(line))
        {
            excluded.insert(number);
        }
        if comment.trim() == "# karva: mutate on" {
            disabled = false;
        }
    }
    let mut visitor = Discovery {
        source,
        tokens: parsed.tokens(),
        edits: Vec::new(),
        scopes: Vec::new(),
        symbols: Vec::new(),
    };
    if let ruff_python_ast::Mod::Module(module) = parsed.syntax() {
        visitor.visit_body(&module.body);
    }
    visitor.edits.sort_by(|left, right| {
        (left.0.start(), left.0.end(), left.1, &left.2).cmp(&(
            right.0.start(),
            right.0.end(),
            right.1,
            &right.2,
        ))
    });
    visitor.edits.dedup();
    let source_fingerprint = fingerprint(&[source.as_bytes()]);
    let mut mutations = Vec::new();
    for (range, operator, replacement) in visitor.edits {
        let start = usize::from(range.start());
        let end = usize::from(range.end());
        let line = source[..start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        let end_line = line
            + source[start..end]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count();
        if excluded.range(line..=end_line).next().is_some()
            || (!filters.operators.is_empty() && !filters.operators.contains(&operator))
        {
            continue;
        }
        let symbol = visitor
            .symbols
            .iter()
            .rev()
            .find(|(scope, _)| scope.contains_range(range))
            .map_or("", |(_, name)| name.as_str());
        if filters
            .symbols
            .as_ref()
            .is_some_and(|patterns| !patterns.is_match(symbol))
            || filters
                .exclude_symbols
                .as_ref()
                .is_some_and(|patterns| patterns.is_match(symbol))
        {
            continue;
        }
        let original = &source[range];
        if original == replacement {
            continue;
        }
        let mut mutated = source.to_owned();
        mutated.replace_range(start..end, &replacement);
        if parse_unchecked(&mutated, options.clone()).has_syntax_errors() {
            continue;
        }
        let operator_name = serde_json::to_string(&operator)?;
        let id = fingerprint(&[
            path.as_str().as_bytes(),
            source_fingerprint.as_bytes(),
            start.to_string().as_bytes(),
            end.to_string().as_bytes(),
            operator_name.as_bytes(),
            original.as_bytes(),
            replacement.as_bytes(),
        ]);
        mutations.push(Mutant {
            id,
            path: path.to_owned(),
            source_fingerprint: source_fingerprint.clone(),
            start,
            end,
            line,
            symbol: symbol.to_owned(),
            operator,
            original: original.to_owned(),
            replacement,
        });
    }
    Ok(mutations)
}

struct Discovery<'source, 'tokens> {
    tokens: &'tokens Tokens,
    source: &'source str,
    edits: Vec<(TextRange, Operator, String)>,
    scopes: Vec<String>,
    symbols: Vec<(TextRange, String)>,
}

impl Discovery<'_, '_> {
    /// Only inspect AST operator spans, excluding unpacking and signatures.
    fn operators(&mut self, range: TextRange) {
        for token in self.tokens.in_range(range) {
            let edit = match token.kind() {
                TokenKind::EqEqual => Some((Operator::Comparison, "!=")),
                TokenKind::NotEqual => Some((Operator::Comparison, "==")),
                TokenKind::Less => Some((Operator::Comparison, "<=")),
                TokenKind::LessEqual => Some((Operator::Comparison, "<")),
                TokenKind::Greater => Some((Operator::Comparison, ">=")),
                TokenKind::GreaterEqual => Some((Operator::Comparison, ">")),
                TokenKind::And => Some((Operator::Boolean, "or")),
                TokenKind::Or => Some((Operator::Boolean, "and")),
                TokenKind::Plus => Some((Operator::Arithmetic, "-")),
                TokenKind::Minus => Some((Operator::Arithmetic, "+")),
                TokenKind::Star => Some((Operator::Arithmetic, "/")),
                TokenKind::Slash => Some((Operator::Arithmetic, "*")),
                TokenKind::DoubleSlash => Some((Operator::Arithmetic, "/")),
                TokenKind::Percent => Some((Operator::Arithmetic, "*")),
                TokenKind::DoubleStar => Some((Operator::Arithmetic, "*")),
                _ => None,
            };
            if let Some((operator, replacement)) = edit {
                self.edits
                    .push((token.range(), operator, replacement.to_owned()));
            }
        }
    }

    fn edit(&mut self, range: TextRange, operator: Operator, replacement: &str) {
        self.edits.push((range, operator, replacement.to_owned()));
    }
}

impl<'ast> Visitor<'ast> for Discovery<'_, '_> {
    fn visit_stmt(&mut self, statement: &'ast Stmt) {
        if matches!(statement, Stmt::Expr(value) if matches!(&*value.value, Expr::StringLiteral(_)))
        {
            return;
        }
        let scope = match statement {
            Stmt::FunctionDef(function) => Some(function.name.as_str()),
            Stmt::ClassDef(class) => Some(class.name.as_str()),
            _ => None,
        };
        if let Some(name) = scope {
            self.scopes.push(name.to_owned());
            self.symbols
                .push((statement.range(), self.scopes.join(".")));
        }
        match statement {
            Stmt::Return(ret) => {
                if let Some(value) = &ret.value {
                    self.edit(
                        value.range(),
                        Operator::Return,
                        if matches!(&**value, Expr::NoneLiteral(_)) {
                            "False"
                        } else {
                            "None"
                        },
                    );
                }
            }
            Stmt::If(branch) => {
                self.edit(branch.test.range(), Operator::ControlFlow, "True");
                self.edit(branch.test.range(), Operator::ControlFlow, "False");
            }
            Stmt::While(branch) => {
                self.edit(branch.test.range(), Operator::ControlFlow, "True");
                self.edit(branch.test.range(), Operator::ControlFlow, "False");
            }
            Stmt::Break(_) => self.edit(statement.range(), Operator::ControlFlow, "continue"),
            Stmt::Continue(_) => self.edit(statement.range(), Operator::ControlFlow, "break"),
            _ => {}
        }
        walk_stmt(self, statement);
        if scope.is_some() {
            self.scopes.pop();
        }
    }

    fn visit_expr(&mut self, expression: &'ast Expr) {
        match expression {
            Expr::BooleanLiteral(boolean) => self.edit(
                expression.range(),
                Operator::Literal,
                if boolean.value { "False" } else { "True" },
            ),
            Expr::NumberLiteral(number) => {
                let zero = match &number.value {
                    Number::Int(value) => value.as_u64() == Some(0),
                    Number::Float(value) => *value == 0.0,
                    Number::Complex { real, imag } => *real == 0.0 && *imag == 0.0,
                };
                self.edit(
                    expression.range(),
                    Operator::Literal,
                    if zero { "1" } else { "0" },
                );
            }
            Expr::StringLiteral(value) => self.edit(
                expression.range(),
                Operator::Literal,
                if value.value.to_str().is_empty() {
                    "'mutated'"
                } else {
                    "''"
                },
            ),
            Expr::UnaryOp(unary) if unary.op == UnaryOp::Not => {
                let replacement = format!("({})", &self.source[unary.operand.range()]);
                self.edit(expression.range(), Operator::Boolean, &replacement);
            }
            Expr::BinOp(binary) => {
                self.operators(TextRange::new(binary.left.end(), binary.right.start()));
            }
            Expr::Compare(compare) => {
                let mut left = &*compare.left;
                for right in &compare.comparators {
                    self.operators(TextRange::new(left.end(), right.start()));
                    left = right;
                }
            }
            Expr::BoolOp(boolean) => {
                for pair in boolean.values.windows(2) {
                    self.operators(TextRange::new(pair[0].end(), pair[1].start()));
                }
            }
            Expr::UnaryOp(unary) => {
                self.operators(TextRange::new(unary.start(), unary.operand.start()));
            }
            _ => {}
        }
        walk_expr(self, expression);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn candidates(source: &str) -> Vec<Mutant> {
        discover(
            Utf8Path::new("sample.py"),
            source,
            PythonVersion::default(),
            &DiscoveryFilters::default(),
        )
        .expect("discover")
    }

    #[rstest]
    #[case("flag = True", Operator::Literal, "False")]
    #[case("flag = a == b", Operator::Comparison, "!=")]
    #[case("flag = a and b", Operator::Boolean, "or")]
    #[case("value = a + b", Operator::Arithmetic, "-")]
    #[case("if flag:\n    pass", Operator::ControlFlow, "False")]
    #[case("def f():\n    return flag", Operator::Return, "None")]
    fn generates_each_operator_family(
        #[case] source: &str,
        #[case] operator: Operator,
        #[case] replacement: &str,
    ) {
        let mutations = candidates(source);
        assert!(
            mutations.iter().any(
                |mutation| mutation.operator == operator && mutation.replacement == replacement
            )
        );
        for mutation in mutations {
            let changed = mutation.apply(source).expect("apply");
            ruff_python_parser::parse_module(&changed).expect("parse");
        }
    }

    #[test]
    fn preserves_source_and_rejects_stale_application() {
        let source = "label = 'café'\r\nflag = True  # keep\r\n";
        let mutations = candidates(source);
        let boolean = mutations
            .iter()
            .find(|mutation| mutation.original == "True")
            .expect("boolean");
        assert_eq!(
            boolean.apply(source).expect("apply"),
            "label = 'café'\r\nflag = False  # keep\r\n"
        );
        assert!(boolean.apply(&format!("{source}\n")).is_err());
        assert_eq!(
            boolean.id,
            candidates(source)
                .iter()
                .find(|mutation| mutation.original == "True")
                .expect("same")
                .id
        );
    }

    #[test]
    fn pragmas_only_come_from_comments_and_respect_blocks() {
        let source = "a = True # pragma: no mutate\n# karva: mutate off\nb = False\n# karva: mutate on\nc = True\ntext = '# karva: mutate off'\nd = False\n";
        let lines: Vec<_> = candidates(source)
            .iter()
            .map(|mutation| mutation.line)
            .collect();
        assert_eq!(lines, [5, 6, 7]);
    }

    #[test]
    fn filters_qualified_symbols_and_operators() {
        let source = "class C:\n    def f(self):\n        return True\n    def g(self):\n        return False\n";
        let filters =
            DiscoveryFilters::new(&["literal".into()], &["C.*".into()], &["*.g".into()], &[])
                .expect("filters");
        let mutations = discover(
            Utf8Path::new("sample.py"),
            source,
            PythonVersion::default(),
            &filters,
        )
        .expect("discover");
        assert_eq!(mutations.len(), 1);
        assert_eq!(mutations[0].symbol, "C.f");
    }

    #[test]
    fn arithmetic_operators_exclude_signatures_and_unpacking() {
        let source = "def f(a, /, b, *, c, **kw):\n    return g(*a, **kw), {**kw}, [*a]\n";
        assert!(
            candidates(source)
                .iter()
                .all(|mutation| mutation.operator != Operator::Arithmetic)
        );
        let source = "result = (a * b) ** (c + d)\n";
        let operators: Vec<_> = candidates(source)
            .into_iter()
            .filter(|mutation| mutation.operator == Operator::Arithmetic)
            .map(|mutation| mutation.original)
            .collect();
        assert_eq!(operators, ["*", "**", "+"]);
    }

    #[test]
    fn docstrings_and_bare_strings_are_excluded_but_string_values_are_mutated() {
        let source = "'module docs'\nclass C:\n    'class docs'\n    def f(self):\n        'function docs'\n        'unused'\n        value = 'payload'\n        return value\n";
        let strings: Vec<_> = candidates(source)
            .into_iter()
            .filter(|mutation| mutation.operator == Operator::Literal)
            .map(|mutation| mutation.original)
            .collect();
        assert_eq!(strings, ["'payload'"]);
    }

    #[rstest]
    fn zero_spellings_change_value(#[values("0", "0.0", "0x0", "0j", "0_0")] literal: &str) {
        let mutations = candidates(&format!("value = {literal}"));
        assert_eq!(mutations.len(), 1);
        assert_eq!(mutations[0].replacement, "1");
    }

    #[test]
    fn reports_invalid_filters_and_source() {
        assert!(DiscoveryFilters::new(&["typo".into()], &[], &[], &[]).is_err());
        assert!(DiscoveryFilters::new(&[], &["[".into()], &[], &[]).is_err());
        assert!(DiscoveryFilters::new(&[], &[], &[], &["[".into()]).is_err());
        assert!(
            discover(
                Utf8Path::new("bad.py"),
                "broken =",
                PythonVersion::default(),
                &DiscoveryFilters::default()
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod snapshots {
    use super::*;

    #[test]
    fn representative_edits() {
        let source = "def choose(a, b):\n    if a < b and not False:\n        return a + 1\n    return 'fallback'\n";
        let mutations = discover(
            Utf8Path::new("sample.py"),
            source,
            PythonVersion::default(),
            &DiscoveryFilters::default(),
        )
        .expect("discover");
        let edits: Vec<_> = mutations
            .iter()
            .map(|mutation| {
                (
                    mutation.line,
                    &mutation.symbol,
                    mutation.operator,
                    &mutation.original,
                    &mutation.replacement,
                )
            })
            .collect();
        insta::assert_debug_snapshot!(edits, @r#"
        [
            (
                2,
                "choose",
                ControlFlow,
                "a < b and not False",
                "False",
            ),
            (
                2,
                "choose",
                ControlFlow,
                "a < b and not False",
                "True",
            ),
            (
                2,
                "choose",
                Comparison,
                "<",
                "<=",
            ),
            (
                2,
                "choose",
                Boolean,
                "and",
                "or",
            ),
            (
                2,
                "choose",
                Boolean,
                "not False",
                "(False)",
            ),
            (
                2,
                "choose",
                Literal,
                "False",
                "True",
            ),
            (
                3,
                "choose",
                Return,
                "a + 1",
                "None",
            ),
            (
                3,
                "choose",
                Arithmetic,
                "+",
                "-",
            ),
            (
                3,
                "choose",
                Literal,
                "1",
                "0",
            ),
            (
                4,
                "choose",
                Literal,
                "'fallback'",
                "''",
            ),
            (
                4,
                "choose",
                Return,
                "'fallback'",
                "None",
            ),
        ]
        "#);
    }
}
