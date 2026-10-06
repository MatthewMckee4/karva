use ruff_python_ast::visitor::{Visitor, walk_expr};
use ruff_python_ast::{Alias, Expr, Stmt, StmtFunctionDef};

/// Statically count the number of test cases a function will expand to once
/// its `@parametrize` decorators are applied.
///
/// Returns `Some(n)` when at least one recognized decorator exists and every
/// `argvalues` expression is a literal list or tuple without unpacking. Returns
/// `None` when no parametrize decorator exists or any `argvalues` is dynamic;
/// callers should treat the function as one opaque scheduling unit.
pub fn count_parametrize_cases(stmt: &StmtFunctionDef) -> Option<usize> {
    let mut total: usize = 1;
    let mut found = false;

    for decorator in &stmt.decorator_list {
        let Expr::Call(call) = &decorator.expression else {
            continue;
        };

        if !is_parametrize_call(call.func.as_ref()) {
            continue;
        }
        found = true;

        let argvalues = argvalues_arg(call)?;
        let count = literal_sequence_len(argvalues)?;

        total = total.checked_mul(count)?;
    }

    found.then_some(total)
}

/// Returns true if `func` resolves to a parametrize reference.
///
/// Matches bare `parametrize`, `pytest.mark.parametrize`, and
/// `karva.tags.parametrize`.
fn is_parametrize_call(func: &Expr) -> bool {
    match func {
        Expr::Name(name) => name.id == "parametrize",
        Expr::Attribute(attr) if attr.attr.id == "parametrize" => {
            let Expr::Attribute(namespace) = attr.value.as_ref() else {
                return false;
            };
            let Expr::Name(root) = namespace.value.as_ref() else {
                return false;
            };
            matches!(
                (root.id.as_str(), namespace.attr.id.as_str()),
                ("pytest", "mark") | ("karva", "tags")
            )
        }
        _ => false,
    }
}

/// Extract the `argvalues` argument from a parametrize call.
///
/// Accepts both `parametrize("x", [1, 2])` (positional) and
/// `parametrize(argnames="x", argvalues=[1, 2])` (keyword).
fn argvalues_arg(call: &ruff_python_ast::ExprCall) -> Option<&Expr> {
    if let Some(expr) = call.arguments.args.get(1) {
        return Some(expr);
    }
    call.arguments
        .keywords
        .iter()
        .find(|kw| kw.arg.as_ref().is_some_and(|id| id.as_str() == "argvalues"))
        .map(|kw| &kw.value)
}

/// Returns the element count of a list or tuple literal, or `None` if the
/// expression isn't a literal sequence we can count statically.
fn literal_sequence_len(expr: &Expr) -> Option<usize> {
    let elements = match expr {
        Expr::List(list) => &list.elts,
        Expr::Tuple(tuple) => &tuple.elts,
        _ => return None,
    };
    (!elements
        .iter()
        .any(|element| matches!(element, Expr::Starred(_))))
    .then_some(elements.len())
}

/// Detects module mark names whose runtime values can add parameter dimensions.
///
/// References inside control flow, helpers, and imports are included conservatively:
/// source collection cannot know which of these bind marks when Python imports the module.
#[expect(
    clippy::redundant_pub_crate,
    reason = "module mark detection is shared only by collector modules"
)]
pub(super) fn has_module_tags(body: &[Stmt]) -> bool {
    /// Records conservative evidence that Python may bind module marks.
    #[derive(Default)]
    struct ModuleTagsVisitor(bool);

    impl<'a> Visitor<'a> for ModuleTagsVisitor {
        fn visit_expr(&mut self, expression: &'a Expr) {
            if matches!(expression, Expr::Name(name) if is_module_tag_name(&name.id)) {
                self.0 = true;
            } else if !self.0 {
                walk_expr(self, expression);
            }
        }

        fn visit_alias(&mut self, alias: &'a Alias) {
            let name = alias.asname.as_ref().unwrap_or(&alias.name);
            self.0 |= is_module_tag_name(name);
        }
    }

    let mut visitor = ModuleTagsVisitor::default();
    visitor.visit_body(body);
    visitor.0
}

fn is_module_tag_name(name: &str) -> bool {
    matches!(name, "pytestmark" | "karva_tag")
}

#[cfg(test)]
mod tests {
    use ruff_python_ast::{Mod, Stmt};
    use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};

    use super::*;

    fn parse_function(source: &str) -> StmtFunctionDef {
        let parsed = parse_unchecked(source, ParseOptions::from(Mode::Module))
            .try_into_module()
            .expect("parse")
            .into_syntax();
        let Mod::Module(module) = Mod::Module(parsed) else {
            unreachable!()
        };
        module
            .body
            .into_iter()
            .find_map(|stmt| match stmt {
                Stmt::FunctionDef(f) => Some(f),
                _ => None,
            })
            .expect("function def")
    }

    #[test]
    fn no_decorators_returns_none() {
        let f = parse_function("def test_x(): pass\n");
        assert_eq!(count_parametrize_cases(&f), None);
    }

    #[test]
    fn unrelated_decorator_returns_none() {
        let f = parse_function("@my_decorator\ndef test_x(): pass\n");
        assert_eq!(count_parametrize_cases(&f), None);
    }

    #[test]
    fn unrelated_parametrize_attribute_returns_none() {
        let f = parse_function("@custom.parametrize('x', [1, 2])\ndef test_x(x): pass\n");
        assert_eq!(count_parametrize_cases(&f), None);
    }

    #[test]
    fn pytest_mark_parametrize_list() {
        let f = parse_function("@pytest.mark.parametrize('x', [1, 2, 3])\ndef test_x(x): pass\n");
        assert_eq!(count_parametrize_cases(&f), Some(3));
    }

    #[test]
    fn karva_tags_parametrize_list_of_tuples() {
        let f = parse_function(
            "@karva.tags.parametrize('a, b', [(1, 2), (3, 4)])\ndef test_x(a, b): pass\n",
        );
        assert_eq!(count_parametrize_cases(&f), Some(2));
    }

    #[test]
    fn bare_parametrize_name() {
        let f = parse_function("@parametrize('x', [1, 2])\ndef test_x(x): pass\n");
        assert_eq!(count_parametrize_cases(&f), Some(2));
    }

    #[test]
    fn keyword_argvalues() {
        let f = parse_function(
            "@pytest.mark.parametrize(argnames='x', argvalues=[1, 2, 3, 4])\ndef test_x(x): pass\n",
        );
        assert_eq!(count_parametrize_cases(&f), Some(4));
    }

    #[test]
    fn stacked_decorators_multiply() {
        let f = parse_function(
            "@pytest.mark.parametrize('a', [1, 2, 3])\n\
             @pytest.mark.parametrize('b', [4, 5])\n\
             def test_x(a, b): pass\n",
        );
        assert_eq!(count_parametrize_cases(&f), Some(6));
    }

    #[test]
    fn tuple_argvalues_counts_outer_elements() {
        let f =
            parse_function("@parametrize('x', ((1, 2), (3, 4), (5, 6)))\ndef test_x(x): pass\n");
        assert_eq!(count_parametrize_cases(&f), Some(3));
    }

    #[test]
    fn dynamic_argvalues_returns_none() {
        let f = parse_function("@parametrize('x', load_cases())\ndef test_x(x): pass\n");
        assert_eq!(count_parametrize_cases(&f), None);
    }

    #[test]
    fn list_comprehension_argvalues_returns_none() {
        let f = parse_function("@parametrize('x', [v for v in values()])\ndef test_x(x): pass\n");
        assert_eq!(count_parametrize_cases(&f), None);
    }

    #[test]
    fn missing_argvalues_returns_none() {
        let f = parse_function("@parametrize('x')\ndef test_x(x): pass\n");
        assert_eq!(count_parametrize_cases(&f), None);
    }

    #[test]
    fn dynamic_decorator_propagates_none() {
        let f = parse_function(
            "@parametrize('a', [1, 2])\n\
             @parametrize('b', dynamic_values())\n\
             def test_x(a, b): pass\n",
        );
        assert_eq!(count_parametrize_cases(&f), None);
    }
}
