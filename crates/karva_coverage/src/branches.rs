//! Compute branch destinations for Python source files.

use std::collections::{BTreeSet, HashSet};
use std::io;
use std::path::Path;

use fs_err as fs;
use ruff_python_ast::helpers::is_docstring_stmt;
use ruff_python_ast::{
    ElifElseClause, ExceptHandler, MatchCase, Stmt, StmtClassDef, StmtFunctionDef, StmtIf,
    StmtMatch,
};
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
use ruff_source_file::LineIndex;
use ruff_text_size::{Ranged, TextSize};

use crate::data::BranchArc;
use crate::executable::pragma_no_cover_lines;

#[expect(
    clippy::redundant_pub_crate,
    reason = "sibling modules use this helper, while unreachable_pub rejects `pub` here"
)]
pub(crate) fn branch_arcs(path: &Path) -> io::Result<BTreeSet<BranchArc>> {
    let source = fs::read_to_string(path)?;
    Ok(branch_arcs_for_source(&source))
}

fn branch_arcs_for_source(source: &str) -> BTreeSet<BranchArc> {
    let Some(parsed) = parse_unchecked(source, ParseOptions::from(Mode::Module)).try_into_module()
    else {
        return BTreeSet::new();
    };
    let line_index = LineIndex::from_source_text(source);
    let pragma_lines = pragma_no_cover_lines(&parsed, source, &line_index);
    let module = parsed.into_syntax();
    let executable = crate::executable::executable_lines_for_source(source);
    let mut collector = BranchCollector {
        line_index: &line_index,
        pragma_lines: &pragma_lines,
        executable: &executable,
        arcs: BTreeSet::new(),
    };
    collector.visit_body(&module.body, None);
    collector.arcs
}

struct BranchCollector<'a> {
    line_index: &'a LineIndex,
    pragma_lines: &'a HashSet<u32>,
    executable: &'a HashSet<u32>,
    arcs: BTreeSet<BranchArc>,
}

impl BranchCollector<'_> {
    fn visit_body(&mut self, body: &[Stmt], next: Option<i32>) {
        let body = skip_docstring(body);
        for (idx, stmt) in body.iter().enumerate() {
            let stmt_next = self
                .first_executable_in_body(&body[idx + 1..])
                .map(line_to_i32)
                .or(next);
            self.visit_stmt(stmt, stmt_next);
        }
    }

    fn visit_stmt(&mut self, stmt: &Stmt, next: Option<i32>) {
        if self.line_has_pragma(stmt_line_offset(stmt)) {
            return;
        }

        match stmt {
            Stmt::FunctionDef(stmt) => self.visit_function_def(stmt),
            Stmt::ClassDef(stmt) => self.visit_class_def(stmt),
            Stmt::If(stmt) => self.visit_if(stmt, next),
            Stmt::For(stmt) => {
                let line = self.line(stmt.range().start());
                self.add_branch(line, [self.first_executable_i32(&stmt.body), next]);
                self.visit_body(&stmt.body, line.map(line_to_i32));
                self.visit_body(&stmt.orelse, next);
            }
            Stmt::While(stmt) => {
                let line = self.line(stmt.range().start());
                if !is_constant_true_while(stmt) {
                    self.add_branch(line, [self.first_executable_i32(&stmt.body), next]);
                }
                self.visit_body(&stmt.body, line.map(line_to_i32));
                self.visit_body(&stmt.orelse, next);
            }
            Stmt::Try(stmt) => {
                self.visit_body(&stmt.body, next);
                for handler in &stmt.handlers {
                    self.visit_except_handler(handler, next);
                }
                self.visit_body(&stmt.orelse, next);
                self.visit_body(&stmt.finalbody, next);
            }
            Stmt::Match(stmt) => self.visit_match(stmt, next),
            _ => {}
        }
    }

    fn visit_function_def(&mut self, stmt: &StmtFunctionDef) {
        let exit = self
            .line(stmt.name.range().start())
            .map(|line| -line_to_i32(line));
        self.visit_body(&stmt.body, exit);
    }

    fn visit_class_def(&mut self, stmt: &StmtClassDef) {
        let exit = self
            .line(stmt.name.range().start())
            .map(|line| -line_to_i32(line));
        self.visit_body(&stmt.body, exit);
    }

    fn visit_if(&mut self, stmt: &StmtIf, next: Option<i32>) {
        let line = self.line(stmt.range().start());
        let alternate = self.if_alternate_target(stmt, next);
        self.add_branch(line, [self.first_executable_i32(&stmt.body), alternate]);
        self.visit_body(&stmt.body, next);

        for (idx, clause) in stmt.elif_else_clauses.iter().enumerate() {
            if self.line_has_pragma(clause.range().start()) {
                continue;
            }
            if clause.test.is_some() {
                let clause_line = self.line(clause.range().start());
                let alternate = self.next_clause_target(&stmt.elif_else_clauses[idx + 1..], next);
                self.add_branch(
                    clause_line,
                    [self.first_executable_i32(&clause.body), alternate],
                );
            }
            self.visit_body(&clause.body, next);
        }
    }

    fn visit_match(&mut self, stmt: &StmtMatch, next: Option<i32>) {
        for (idx, case) in stmt.cases.iter().enumerate() {
            if self.line_has_pragma(case.range().start()) {
                continue;
            }
            if !case.pattern.is_irrefutable() || case.guard.is_some() {
                let line = self.line(case.range().start());
                let alternate = self.next_match_case_target(&stmt.cases[idx + 1..], next);
                self.add_branch(line, [self.first_executable_i32(&case.body), alternate]);
            }
            self.visit_body(&case.body, next);
        }
    }

    fn visit_except_handler(&mut self, handler: &ExceptHandler, next: Option<i32>) {
        match handler {
            ExceptHandler::ExceptHandler(handler) => self.visit_body(&handler.body, next),
        }
    }

    fn if_alternate_target(&self, stmt: &StmtIf, next: Option<i32>) -> Option<i32> {
        self.next_clause_target(&stmt.elif_else_clauses, next)
    }

    fn next_clause_target(&self, clauses: &[ElifElseClause], next: Option<i32>) -> Option<i32> {
        if clauses.is_empty() {
            next
        } else {
            clauses.iter().find_map(|clause| self.clause_target(clause))
        }
    }

    fn clause_target(&self, clause: &ElifElseClause) -> Option<i32> {
        if self.line_has_pragma(clause.range().start()) {
            return None;
        }
        if clause.test.is_some() {
            self.line(clause.range().start()).map(line_to_i32)
        } else {
            self.first_executable_i32(&clause.body)
        }
    }

    fn match_case_target(&self, case: &MatchCase) -> Option<i32> {
        if self.line_has_pragma(case.range().start()) {
            return None;
        }
        self.line(case.range().start()).map(line_to_i32)
    }

    fn next_match_case_target(&self, cases: &[MatchCase], next: Option<i32>) -> Option<i32> {
        if cases.is_empty() {
            next
        } else {
            cases.iter().find_map(|case| self.match_case_target(case))
        }
    }

    fn first_executable_i32(&self, body: &[Stmt]) -> Option<i32> {
        self.first_executable_in_body(body).map(line_to_i32)
    }

    fn first_executable_in_body(&self, body: &[Stmt]) -> Option<u32> {
        skip_docstring(body)
            .iter()
            .filter(|stmt| !self.line_has_pragma(stmt_line_offset(stmt)))
            .find_map(|stmt| self.line(stmt_line_offset(stmt)))
            .filter(|line| self.executable.contains(line))
    }

    fn add_branch<const N: usize>(&mut self, from: Option<u32>, targets: [Option<i32>; N]) {
        let Some(from) = from else {
            return;
        };
        let targets: BTreeSet<i32> = targets.into_iter().flatten().collect();
        if targets.len() < 2 {
            return;
        }
        for to in targets {
            self.arcs.insert(BranchArc {
                from: line_to_i32(from),
                to,
            });
        }
    }

    fn line(&self, offset: TextSize) -> Option<u32> {
        u32::try_from(self.line_index.line_index(offset).get()).ok()
    }

    fn line_has_pragma(&self, offset: TextSize) -> bool {
        self.line(offset)
            .is_some_and(|line| self.pragma_lines.contains(&line))
    }
}

fn stmt_line_offset(stmt: &Stmt) -> TextSize {
    match stmt {
        Stmt::FunctionDef(stmt) => stmt.name.range().start(),
        Stmt::ClassDef(stmt) => stmt.name.range().start(),
        _ => stmt.range().start(),
    }
}

fn skip_docstring(body: &[Stmt]) -> &[Stmt] {
    let start = usize::from(body.first().is_some_and(is_docstring_stmt));
    &body[start..]
}

fn line_to_i32(line: u32) -> i32 {
    i32::try_from(line).unwrap_or(i32::MAX)
}

fn is_constant_true_while(stmt: &ruff_python_ast::StmtWhile) -> bool {
    matches!(&*stmt.test, ruff_python_ast::Expr::BooleanLiteral(value) if value.value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arcs(source: &str) -> Vec<(i32, i32)> {
        branch_arcs_for_source(source)
            .into_iter()
            .map(|arc| (arc.from, arc.to))
            .collect()
    }

    #[test]
    fn if_else_arcs_point_to_bodies() {
        let source = "\
def f(x):
    if x:
        return 1
    return 0
";

        assert_eq!(arcs(source), vec![(2, 3), (2, 4)]);
    }

    #[test]
    fn if_without_fallthrough_uses_function_exit() {
        let source = "\
def f(x):
    if x:
        return 1
";

        assert_eq!(arcs(source), vec![(2, -1), (2, 3)]);
    }

    #[test]
    fn loop_arcs_point_to_body_and_exit() {
        let source = "\
def f(items):
    for item in items:
        print(item)
    return None
";

        assert_eq!(arcs(source), vec![(2, 3), (2, 4)]);
    }

    #[test]
    fn match_case_arcs_point_to_body_and_next_case() {
        let source = "\
def f(x):
    match x:
        case 1:
            return 1
        case _:
            return 0
";

        assert_eq!(arcs(source), vec![(3, 4), (3, 5)]);
    }

    #[test]
    fn pragma_excluded_choice_removes_branch() {
        let source = "\
def f(x):
    if x:
        return 1
    else:  # pragma: no cover
        return 0
";

        assert!(arcs(source).is_empty());
    }
}
