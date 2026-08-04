//! Static validation for custom tag names before Python modules are imported.

use std::collections::BTreeMap;

use ruff_python_ast::visitor::source_order::{self, SourceOrderVisitor};
use ruff_python_ast::{Expr, StmtFunctionDef};
use ruff_text_size::TextRange;

const KARVA_BUILTINS: &[&str] = &[
    "expect_fail",
    "fail_slow",
    "parametrize",
    "skip",
    "timeout",
    "use_fixtures",
];
const PYTEST_BUILTINS: &[&str] = &[
    "parametrize",
    "skip",
    "skipif",
    "timeout",
    "usefixtures",
    "xfail",
];

/// One custom tag reference absent from the project registry.
pub struct UnknownTag {
    pub(crate) name: String,
    pub(crate) range: TextRange,
    pub(crate) suggestion: Option<String>,
}

/// Finds unregistered `karva.tags.*` and `pytest.mark.*` references in decorators.
pub fn unknown_tags(
    function: &StmtFunctionDef,
    registered: &BTreeMap<String, String>,
) -> Vec<UnknownTag> {
    let mut visitor = TagVisitor {
        registered,
        unknown: Vec::new(),
    };
    for decorator in &function.decorator_list {
        visitor.visit_expr(&decorator.expression);
    }
    visitor.unknown
}

struct TagVisitor<'a> {
    registered: &'a BTreeMap<String, String>,
    unknown: Vec<UnknownTag>,
}

impl SourceOrderVisitor<'_> for TagVisitor<'_> {
    fn visit_expr(&mut self, expression: &'_ Expr) {
        if let Expr::Attribute(attribute) = expression
            && let Expr::Attribute(namespace) = &*attribute.value
            && let Expr::Name(root) = &*namespace.value
        {
            let name = attribute.attr.id.as_str();
            let builtins = match (root.id.as_str(), namespace.attr.id.as_str()) {
                ("karva", "tags") => Some(KARVA_BUILTINS),
                ("pytest", "mark") => Some(PYTEST_BUILTINS),
                _ => None,
            };
            if let Some(builtins) = builtins
                && !builtins.contains(&name)
                && !self.registered.contains_key(name)
            {
                self.unknown.push(UnknownTag {
                    name: name.to_string(),
                    range: attribute.attr.range,
                    suggestion: unique_suggestion(name, self.registered.keys()),
                });
            }
        }
        source_order::walk_expr(self, expression);
    }
}

fn unique_suggestion<'a>(
    unknown: &str,
    registered: impl Iterator<Item = &'a String>,
) -> Option<String> {
    let mut matches = registered.filter(|candidate| is_single_typo(unknown, candidate));
    let suggestion = matches.next()?.clone();
    matches.next().is_none().then_some(suggestion)
}

fn is_single_typo(left: &str, right: &str) -> bool {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    match left.len().cmp(&right.len()) {
        std::cmp::Ordering::Equal => {
            let differences: Vec<usize> = left
                .iter()
                .zip(&right)
                .enumerate()
                .filter_map(|(index, (left, right))| (left != right).then_some(index))
                .collect();
            differences.len() == 1
                || matches!(differences.as_slice(), [first, second]
                    if *second == *first + 1
                        && left[*first] == right[*second]
                        && left[*second] == right[*first])
        }
        std::cmp::Ordering::Less => one_insertion_away(&left, &right),
        std::cmp::Ordering::Greater => one_insertion_away(&right, &left),
    }
}

fn one_insertion_away(shorter: &[char], longer: &[char]) -> bool {
    if longer.len() != shorter.len() + 1 {
        return false;
    }
    let mismatch = shorter
        .iter()
        .zip(longer)
        .position(|(left, right)| left != right)
        .unwrap_or(shorter.len());
    shorter[mismatch..] == longer[mismatch + 1..]
}

#[cfg(test)]
mod tests {
    use super::is_single_typo;

    #[test]
    fn recognizes_common_single_edit_typos() {
        assert!(is_single_typo("intergation", "integration"));
        assert!(is_single_typo("integraton", "integration"));
        assert!(is_single_typo("integrations", "integration"));
        assert!(is_single_typo("integretion", "integration"));
        assert!(!is_single_typo("unit", "integration"));
    }
}
