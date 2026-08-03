use std::collections::{BTreeSet, HashSet};
use std::io;
use std::path::Path;

use fs_err as fs;
use regex::Regex;
use ruff_python_ast::helpers::is_docstring_stmt;
use ruff_python_ast::{
    ElifElseClause, ExceptHandler, Expr, MatchCase, Stmt, StmtClassDef, StmtFunctionDef, StmtIf,
    StmtMatch,
};
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
use ruff_source_file::LineIndex;
use ruff_text_size::{Ranged, TextSize};

use crate::data::BranchArc;
use crate::executable::{
    CoverageExclusions, comment_lines_matching, pattern_lines, pragma_no_cover_lines,
};

#[derive(Clone, Debug, Default)]
/// Compiled expressions identifying intentionally partial branch lines.
#[expect(
    clippy::redundant_pub_crate,
    reason = "tracer carries this type across private sibling modules"
)]
pub(super) struct CoveragePartials(Vec<Regex>);

impl CoveragePartials {
    /// Compiles configured partial-branch expressions before collection begins.
    pub(super) fn new(patterns: &[String]) -> anyhow::Result<Self> {
        patterns
            .iter()
            .map(|pattern| {
                Regex::new(pattern).map_err(|error| {
                    anyhow::anyhow!("invalid coverage partial-branch pattern `{pattern}`: {error}")
                })
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Self)
    }
}

#[expect(
    clippy::redundant_pub_crate,
    reason = "tracer uses this helper across private sibling modules"
)]
pub(crate) fn branch_analysis_with_exclusions(
    path: &Path,
    exclusions: &CoverageExclusions,
    partials: &CoveragePartials,
) -> io::Result<(BTreeSet<BranchArc>, BTreeSet<u32>)> {
    let source = fs::read_to_string(path)?;
    Ok(branch_analysis_for_source(&source, exclusions, partials))
}

#[cfg(test)]
fn branch_arcs_for_source(source: &str) -> BTreeSet<BranchArc> {
    branch_analysis_for_source(
        source,
        &CoverageExclusions::default(),
        &CoveragePartials::default(),
    )
    .0
}

#[cfg(test)]
fn branch_arcs_for_source_with_exclusions(
    source: &str,
    exclusions: &CoverageExclusions,
) -> BTreeSet<BranchArc> {
    branch_analysis_for_source(source, exclusions, &CoveragePartials::default()).0
}

fn branch_analysis_for_source(
    source: &str,
    exclusions: &CoverageExclusions,
    partials: &CoveragePartials,
) -> (BTreeSet<BranchArc>, BTreeSet<u32>) {
    let Some(parsed) = parse_unchecked(source, ParseOptions::from(Mode::Module)).try_into_module()
    else {
        return (BTreeSet::new(), BTreeSet::new());
    };
    let line_index = LineIndex::from_source_text(source);
    let mut excluded_lines = pragma_no_cover_lines(&parsed, source, &line_index);
    excluded_lines.extend(pattern_lines(source, &line_index, exclusions.patterns()));
    let partial_lines = partial_branch_lines(&parsed, source, &line_index, partials);
    let module = parsed.into_syntax();
    let (executable, built_in_excluded) =
        crate::executable::executable_lines_for_source_with_exclusions(source, exclusions);
    excluded_lines.extend(built_in_excluded);
    let mut collector = BranchCollector {
        line_index: &line_index,
        excluded_lines: &excluded_lines,
        executable: &executable,
        arcs: BTreeSet::new(),
    };
    collector.visit_body(&module.body, None);
    let branch_lines = branch_lines(&collector.arcs);
    let partial_lines = partial_lines
        .into_iter()
        .filter(|line| branch_lines.contains(line))
        .collect();
    (collector.arcs, partial_lines)
}

fn partial_branch_lines<T>(
    parsed: &ruff_python_parser::Parsed<T>,
    source: &str,
    line_index: &LineIndex,
    partials: &CoveragePartials,
) -> HashSet<u32> {
    let mut lines = comment_lines_matching(parsed, source, line_index, is_pragma_no_branch);
    lines.extend(pattern_lines(source, line_index, &partials.0));
    lines
}

fn is_pragma_no_branch(comment: &str) -> bool {
    let body = comment.strip_prefix('#').unwrap_or(comment).trim();
    body.to_ascii_lowercase().contains("pragma: no branch")
}

fn branch_lines(arcs: &BTreeSet<BranchArc>) -> HashSet<u32> {
    let mut counts = std::collections::HashMap::new();
    for arc in arcs {
        if let Ok(line) = u32::try_from(arc.from) {
            *counts.entry(line).or_insert(0usize) += 1;
        }
    }
    counts
        .into_iter()
        .filter_map(|(line, count)| (count > 1).then_some(line))
        .collect()
}

/// Walks nested Python bodies while carrying each statement's fallthrough target.
struct BranchCollector<'a> {
    line_index: &'a LineIndex,
    excluded_lines: &'a HashSet<u32>,
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
        if !matches!(&*stmt.test, Expr::BooleanLiteral(_)) {
            self.add_branch(line, [self.first_executable_i32(&stmt.body), alternate]);
        }
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
            .is_some_and(|line| self.excluded_lines.contains(&line))
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

    #[test]
    fn configured_excluded_choice_removes_branch() {
        let source = "\
def f(x):
    if x:
        return 1
    else:
        return 0
";
        let exclusions = CoverageExclusions::new(&["else:".to_owned()]).expect("valid exclusion");

        assert!(branch_arcs_for_source_with_exclusions(source, &exclusions).is_empty());
    }

    #[test]
    fn pragma_marks_branch_as_intentionally_partial() {
        let source = "\
def f(x):
    if x:  # pragma: no branch
        return 1
    return 0
";

        let (arcs, partial) = branch_analysis_for_source(
            source,
            &CoverageExclusions::default(),
            &CoveragePartials::default(),
        );

        assert_eq!(arcs.len(), 2);
        assert_eq!(partial, BTreeSet::from([2]));
    }

    #[test]
    fn configured_pattern_marks_branch_as_intentionally_partial() {
        let source = "\
def f(x):
    if x:
        return 1
    return 0
";
        let partials = CoveragePartials::new(&["if x:".to_owned()]).expect("valid partial");

        let (_, partial) =
            branch_analysis_for_source(source, &CoverageExclusions::default(), &partials);

        assert_eq!(partial, BTreeSet::from([2]));
    }

    #[test]
    fn constant_if_and_type_checking_are_not_branches() {
        let source = "\
from typing import TYPE_CHECKING
if True:
    value = 1
if TYPE_CHECKING:
    if unknown:
        value = 2
";

        assert!(arcs(source).is_empty());
    }
}
