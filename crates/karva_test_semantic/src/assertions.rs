//! Rust-owned assertion instrumentation for first-party Python modules.
//!
//! Ruff identifies assertion and operand ranges. Rust applies source edits for
//! captures, then uses `CPython`'s standard AST only for statement cleanup and
//! compilation. This keeps Python's compiler semantics while avoiding a second
//! expression-tree implementation in Rust.
//!
//! Unchanged modules reuse hash-validated bytecode in a separate Karva cache.
//! Cached modules reconstruct diagnostic metadata only after a failure; only
//! a cache miss needs the Python AST. Assertions without useful evaluated values retain
//! native loading and compilation.

mod cache;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

use camino::Utf8Path;
use pyo3::class::basic::CompareOp;
use pyo3::prelude::*;
use pyo3::types::{
    PyAnyMethods, PyBool, PyBytes, PyDict, PyDictMethods, PyFloat, PyFrozenSet, PyFrozenSetMethods,
    PyInt, PyList, PyListMethods, PySet, PySetMethods, PyString, PyStringMethods, PyTuple,
    PyTupleMethods,
};
use ruff_python_ast::visitor::{Visitor, walk_stmt};
use ruff_python_ast::{CmpOp, Expr, PythonVersion, Stmt};
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
use ruff_text_size::{Ranged, TextRange};

const VALUE_PREFIX: &str = "_karva_value_";
const MISSING_PREFIX: &str = "_karva_missing";

// Entries live for the worker process and are keyed by canonical filename;
// imports replace a file's metadata atomically before its code can run.
static REGISTRY: OnceLock<RwLock<HashMap<String, RegistryEntry>>> = OnceLock::new();

fn registry() -> &'static RwLock<HashMap<String, RegistryEntry>> {
    REGISTRY.get_or_init(|| RwLock::new(HashMap::new()))
}

#[derive(Clone, Debug)]
struct Location {
    /// `CPython`'s one-based source line and UTF-8 byte column.
    line: u32,
    column: u32,
}

#[derive(Clone, Debug)]
struct CaptureMetadata {
    /// Ruff byte offsets into the original source, used only while rewriting.
    range: TextRange,
    label: String,
    name: String,
    optional: bool,
    literal: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ComparisonKind {
    Other,
    Equality,
    Comparison,
    Identity,
}

#[derive(Clone, Debug)]
struct AssertionMetadata {
    /// The compiled `Assert` location; `test_location` selects same-line failures.
    location: Location,

    /// Inclusive original source line span used when traceback points into a multiline assert.
    end_line: u32,
    test_location: Location,
    captures: Vec<CaptureMetadata>,
    comparison: ComparisonKind,
    missing_name: String,
}

/// Cached modules retain source until their first failed assertion needs an
/// explanation. Materialization uses Rust alone while holding the registry lock.
enum RegistryEntry {
    Ready(Vec<AssertionMetadata>),
    Deferred {
        source: String,
        python_version: PythonVersion,
        missing_name: String,
    },
}

#[derive(Debug)]
struct AssertionPlan {
    statement_range: TextRange,
    test_range: TextRange,
    captures: Vec<CaptureMetadata>,
    comparison: ComparisonKind,
    instrumented: bool,
}

struct CollectedPlans {
    plans: Vec<AssertionPlan>,
}

struct EvaluatedCapture<'py> {
    label: String,
    value: Bound<'py, PyAny>,
    literal: bool,
    optional: bool,
}

struct AssertionCollector<'a> {
    source: &'a str,
    names: HashSet<&'a str>,
    plans: Vec<AssertionPlan>,
    value_index: usize,
}

impl<'a> AssertionCollector<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            names: HashSet::new(),
            plans: Vec::new(),
            value_index: 0,
        }
    }

    fn collect_names(&mut self) {
        for token in self
            .source
            .split(|character: char| !(character == '_' || character.is_ascii_alphanumeric()))
        {
            if (token.starts_with(VALUE_PREFIX) || token.starts_with(MISSING_PREFIX))
                && token
                    .chars()
                    .next()
                    .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
            {
                self.names.insert(token);
            }
        }
    }

    fn next_name(&mut self) -> String {
        loop {
            let name = format!("{VALUE_PREFIX}{}", self.value_index);
            self.value_index += 1;
            if !self.names.contains(name.as_str()) {
                return name;
            }
        }
    }

    fn missing_name(&self) -> String {
        let mut suffix = 0;
        loop {
            let candidate = if suffix == 0 {
                MISSING_PREFIX.to_string()
            } else {
                format!("{MISSING_PREFIX}_{suffix}")
            };
            if !self.names.contains(candidate.as_str()) {
                return candidate;
            }
            suffix += 1;
        }
    }

    fn collect_assert(&mut self, assertion: &ruff_python_ast::StmtAssert) {
        let comparison = match assertion.test.as_ref() {
            Expr::Compare(compare)
                if compare
                    .ops
                    .iter()
                    .any(|op| matches!(op, CmpOp::Is | CmpOp::IsNot)) =>
            {
                ComparisonKind::Identity
            }
            Expr::Compare(compare) if compare.ops.iter().all(|op| matches!(op, CmpOp::Eq)) => {
                ComparisonKind::Equality
            }
            Expr::Compare(_) => ComparisonKind::Comparison,
            _ => ComparisonKind::Other,
        };
        let mut captures = Vec::new();
        self.collect_expr(&assertion.test, false, &mut captures);
        let instrumented = captures.iter().any(|capture| !capture.literal)
            || (comparison == ComparisonKind::Equality
                && literal_equality_needs_diagnostics(&assertion.test));
        self.plans.push(AssertionPlan {
            statement_range: assertion.range(),
            test_range: assertion.test.range(),
            captures,
            comparison,
            instrumented,
        });
    }

    fn collect_expr(
        &mut self,
        expression: &Expr,
        may_skip: bool,
        captures: &mut Vec<CaptureMetadata>,
    ) {
        match expression {
            Expr::BoolOp(boolean) => {
                for (index, value) in boolean.values.iter().enumerate() {
                    self.collect_expr(value, may_skip || index > 0, captures);
                }
            }
            Expr::Compare(compare) => {
                for (index, operand) in compare.operands.iter().enumerate() {
                    self.capture(operand, may_skip || index > 1, captures);
                }
            }
            Expr::UnaryOp(unary) if unary.op == ruff_python_ast::UnaryOp::Not => {
                self.collect_expr(&unary.operand, may_skip, captures);
            }
            _ => self.capture(expression, may_skip, captures),
        }
    }

    fn capture(&mut self, expression: &Expr, optional: bool, captures: &mut Vec<CaptureMetadata>) {
        captures.push(CaptureMetadata {
            range: expression.range(),
            label: self.source_slice(expression.range()).trim().to_string(),
            name: self.next_name(),
            optional,
            literal: is_literal(expression),
        });
    }

    fn source_slice(&self, range: TextRange) -> &str {
        &self.source[range.start().to_usize()..range.end().to_usize()]
    }
}

impl<'a> Visitor<'a> for AssertionCollector<'a> {
    fn visit_stmt(&mut self, statement: &'a Stmt) {
        if let Stmt::Assert(assertion) = statement {
            self.collect_assert(assertion);
        } else {
            walk_stmt(self, statement);
        }
    }
}

fn is_literal(expression: &Expr) -> bool {
    match expression {
        Expr::BooleanLiteral(_)
        | Expr::NoneLiteral(_)
        | Expr::EllipsisLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::StringLiteral(_)
        | Expr::BytesLiteral(_) => true,
        Expr::List(list) => list.elts.iter().all(is_literal),
        Expr::Tuple(tuple) => tuple.elts.iter().all(is_literal),
        Expr::Set(set) => set.elts.iter().all(is_literal),
        Expr::Dict(dictionary) => dictionary
            .items
            .iter()
            .all(|item| item.key.as_ref().is_none_or(is_literal) && is_literal(&item.value)),
        _ => false,
    }
}

fn collect_plans(source: &str, python_version: PythonVersion) -> Option<CollectedPlans> {
    let options = ParseOptions::from(Mode::Module).with_target_version(python_version);
    let parsed = parse_unchecked(source, options).try_into_module()?;
    let mut collector = AssertionCollector::new(source);
    collector.collect_names();
    collector.visit_body(parsed.suite());
    Some(CollectedPlans {
        plans: collector.plans,
    })
}

fn missing_name_for_source(source: &str) -> String {
    let mut collector = AssertionCollector::new(source);
    collector.collect_names();
    collector.missing_name()
}

fn literal_equality_needs_diagnostics(expression: &Expr) -> bool {
    let Expr::Compare(compare) = expression else {
        return false;
    };
    compare.operands.iter().any(|expression| {
        matches!(
            expression,
            Expr::List(_)
                | Expr::Tuple(_)
                | Expr::Dict(_)
                | Expr::Set(_)
                | Expr::StringLiteral(_)
                | Expr::BytesLiteral(_)
        )
    })
}

struct Edit {
    range: TextRange,
    replacement: String,
    prefix_bytes: usize,
    suffix_bytes: usize,
}

struct ColumnEvent {
    original_column: usize,
    inserted_bytes: usize,
}

struct SourceMap {
    lines: HashMap<u32, Vec<ColumnEvent>>,
}

impl SourceMap {
    fn from_edits(source: &str, edits: &[Edit]) -> Self {
        let line_starts = source_line_starts(source);
        let mut lines: HashMap<u32, Vec<ColumnEvent>> = HashMap::new();
        for edit in edits {
            let start = edit.range.start().to_usize();
            let end = edit.range.end().to_usize();
            let (start_line, start_column) = source_location(&line_starts, start);
            let (end_line, end_column) = source_location(&line_starts, end);
            let original = &source[start..end];
            if original.is_empty() {
                Self::add_event(
                    &mut lines,
                    start_line,
                    start_column,
                    edit.prefix_bytes + edit.suffix_bytes,
                );
                continue;
            }
            Self::add_event(&mut lines, start_line, start_column, edit.prefix_bytes);
            Self::add_event(&mut lines, end_line, end_column, edit.suffix_bytes);
        }
        for events in lines.values_mut() {
            events.sort_by_key(|event| event.original_column);
            let mut merged: Vec<ColumnEvent> = Vec::with_capacity(events.len());
            for event in events.drain(..) {
                if let Some(previous) = merged.last_mut()
                    && previous.original_column == event.original_column
                {
                    previous.inserted_bytes += event.inserted_bytes;
                } else {
                    merged.push(event);
                }
            }
            *events = merged;
        }
        Self { lines }
    }

    fn add_event(
        lines: &mut HashMap<u32, Vec<ColumnEvent>>,
        line: u32,
        original_column: usize,
        inserted_bytes: usize,
    ) {
        if inserted_bytes > 0 {
            lines.entry(line).or_default().push(ColumnEvent {
                original_column,
                inserted_bytes,
            });
        }
    }

    fn map_column(&self, line: u32, transformed_column: u32) -> u32 {
        let mut delta = 0;
        for event in self.lines.get(&line).into_iter().flatten() {
            let transformed_start = event.original_column + delta;
            if (transformed_column as usize) < transformed_start {
                break;
            }
            if (transformed_column as usize) < transformed_start + event.inserted_bytes {
                return u32::try_from(event.original_column).unwrap_or(u32::MAX);
            }
            delta += event.inserted_bytes;
        }
        u32::try_from((transformed_column as usize).saturating_sub(delta)).unwrap_or(u32::MAX)
    }
}

fn source_line_starts(source: &str) -> Vec<usize> {
    let mut line_starts = vec![0];
    line_starts.extend(
        source
            .bytes()
            .enumerate()
            .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
    );
    line_starts
}

fn source_location(line_starts: &[usize], offset: usize) -> (u32, usize) {
    let line = line_starts
        .binary_search(&offset)
        .unwrap_or_else(|index| index.saturating_sub(1));
    (
        u32::try_from(line + 1).unwrap_or(u32::MAX),
        offset - line_starts[line],
    )
}

fn rewrite_source(
    source: &str,
    plans: &[AssertionPlan],
    missing_name: &str,
) -> (String, SourceMap) {
    let mut edits = Vec::new();
    for plan in plans {
        if !plan.instrumented {
            continue;
        }
        for capture in &plan.captures {
            let original =
                &source[capture.range.start().to_usize()..capture.range.end().to_usize()];
            edits.push(Edit {
                range: capture.range,
                replacement: format!("({} := ({}))", capture.name, original),
                prefix_bytes: format!("({} := (", capture.name).len(),
                suffix_bytes: 2,
            });
        }
        let optional = plan
            .captures
            .iter()
            .filter(|capture| capture.optional)
            .map(|capture| {
                format!(
                    "(({} := {missing_name}) is not {missing_name})",
                    capture.name
                )
            })
            .collect::<Vec<_>>();
        if !optional.is_empty() {
            edits.push(Edit {
                range: TextRange::at(plan.test_range.start(), 0.into()),
                replacement: format!("{} or ", optional.join(" or ")),
                prefix_bytes: optional.join(" or ").len() + 4,
                suffix_bytes: 0,
            });
        }
    }
    let source_map = SourceMap::from_edits(source, &edits);
    edits.sort_by(|left, right| {
        left.range
            .start()
            .cmp(&right.range.start())
            .then_with(|| left.range.len().cmp(&right.range.len()))
    });
    let added = edits
        .iter()
        .map(|edit| {
            edit.replacement
                .len()
                .saturating_sub(edit.range.len().to_usize())
        })
        .sum();
    let mut rewritten = String::with_capacity(source.len().saturating_add(added));
    let mut cursor = 0;
    for edit in edits {
        let start = edit.range.start().to_usize();
        let end = edit.range.end().to_usize();
        if start < cursor {
            continue;
        }
        rewritten.push_str(&source[cursor..start]);
        rewritten.push_str(&edit.replacement);
        cursor = end;
    }
    rewritten.push_str(&source[cursor..]);
    (rewritten, source_map)
}

fn node_kind(node: &Bound<'_, PyAny>) -> PyResult<String> {
    node.getattr("__class__")?.getattr("__name__")?.extract()
}

fn kwargs<'py>(
    py: Python<'py>,
    fields: &[(&str, Bound<'py, PyAny>)],
) -> PyResult<Bound<'py, PyDict>> {
    let result = PyDict::new(py);
    for (name, value) in fields {
        result.set_item(*name, value)?;
    }
    Ok(result)
}

fn construct<'py>(
    py: Python<'py>,
    ast: &Bound<'py, PyModule>,
    name: &str,
    fields: &[(&str, Bound<'py, PyAny>)],
) -> PyResult<Bound<'py, PyAny>> {
    ast.getattr(name)?.call((), Some(&kwargs(py, fields)?))
}

fn context<'py>(ast: &Bound<'py, PyModule>, name: &str) -> PyResult<Bound<'py, PyAny>> {
    ast.getattr(name)?.call0()
}

fn make_name<'py>(
    py: Python<'py>,
    ast: &Bound<'py, PyModule>,
    name: &str,
    ctx: &str,
) -> PyResult<Bound<'py, PyAny>> {
    construct(
        py,
        ast,
        "Name",
        &[
            ("id", name.into_pyobject(py)?.into_any()),
            ("ctx", context(ast, ctx)?),
        ],
    )
}

fn transform_assert<'py>(
    py: Python<'py>,
    ast: &Bound<'py, PyModule>,
    assertion: Bound<'py, PyAny>,
    plan: &AssertionPlan,
) -> PyResult<Bound<'py, PyAny>> {
    let targets = plan
        .captures
        .iter()
        .map(|capture| {
            let name = make_name(py, ast, &capture.name, "Del")?;
            ast.getattr("copy_location")?
                .call1((name, assertion.clone()))
        })
        .collect::<PyResult<Vec<_>>>()?;
    let delete = construct(
        py,
        ast,
        "Delete",
        &[("targets", targets.into_pyobject(py)?.into_any())],
    )?;
    let delete = ast
        .getattr("copy_location")?
        .call1((delete, assertion.clone()))?;
    let debug = make_name(py, ast, "__debug__", "Load")?;
    let debug = ast
        .getattr("copy_location")?
        .call1((debug, assertion.clone()))?;
    let wrapper = construct(
        py,
        ast,
        "If",
        &[
            ("test", debug),
            (
                "body",
                vec![assertion.clone(), delete]
                    .into_pyobject(py)?
                    .into_any(),
            ),
            (
                "orelse",
                Vec::<Bound<'_, PyAny>>::new().into_pyobject(py)?.into_any(),
            ),
        ],
    )?;
    ast.getattr("copy_location")?.call1((wrapper, assertion))
}

fn replace_statement_lists<'py>(
    py: Python<'py>,
    ast: &Bound<'py, PyModule>,
    node: &Bound<'py, PyAny>,
    source_map: &SourceMap,
    assertion_lines: &BTreeSet<u32>,
    plans: &[AssertionPlan],
    next: &mut usize,
) -> PyResult<()> {
    if let Some(start) = node
        .getattr("lineno")
        .ok()
        .and_then(|value| value.extract::<u32>().ok())
    {
        let end = node
            .getattr("end_lineno")
            .ok()
            .and_then(|value| value.extract::<u32>().ok())
            .unwrap_or(start);
        if assertion_lines.range(start..=end).next().is_none() {
            return Ok(());
        }
    }
    remap_location(node, source_map, "lineno", "col_offset")?;
    remap_location(node, source_map, "end_lineno", "end_col_offset")?;
    let fields = ast
        .getattr("iter_fields")?
        .call1((node,))?
        .cast_into::<pyo3::types::PyIterator>()?;
    for field in fields {
        let tuple = field?.cast_into::<pyo3::types::PyTuple>()?;
        let value = tuple.get_item(1)?;
        if let Ok(list) = value.clone().cast_into::<PyList>() {
            let items = list.iter().collect::<Vec<_>>();
            for (index, item) in items.into_iter().enumerate() {
                if node_kind(&item)? == "Assert" {
                    let Some(plan) = plans.get(*next) else {
                        continue;
                    };
                    replace_statement_lists(
                        py,
                        ast,
                        &item,
                        source_map,
                        assertion_lines,
                        plans,
                        next,
                    )?;
                    if plan.instrumented {
                        list.set_item(index, transform_assert(py, ast, item, plan)?)?;
                    }
                    *next += 1;
                } else if item.hasattr("_fields")? {
                    replace_statement_lists(
                        py,
                        ast,
                        &item,
                        source_map,
                        assertion_lines,
                        plans,
                        next,
                    )?;
                }
            }
        } else if value.hasattr("_fields")? {
            replace_statement_lists(py, ast, &value, source_map, assertion_lines, plans, next)?;
        }
    }
    Ok(())
}

fn remap_location(
    node: &Bound<'_, PyAny>,
    source_map: &SourceMap,
    line_name: &str,
    column_name: &str,
) -> PyResult<()> {
    let Ok(line) = node
        .getattr(line_name)
        .and_then(|value| value.extract::<u32>())
    else {
        return Ok(());
    };
    let Ok(column) = node
        .getattr(column_name)
        .and_then(|value| value.extract::<u32>())
    else {
        return Ok(());
    };
    node.setattr(column_name, source_map.map_column(line, column))
}

fn compile_assertions<'py>(
    py: Python<'py>,
    ast: &Bound<'py, PyModule>,
    source: &str,
    filename: &str,
    plans: &[AssertionPlan],
    missing_name: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let (rewritten, source_map) = rewrite_source(source, plans, missing_name);
    let tree = ast.getattr("parse")?.call((rewritten, filename), None)?;
    let mut next = 0;
    let line_starts = source_line_starts(source);
    let assertion_lines = plans
        .iter()
        .map(|plan| source_location(&line_starts, plan.test_range.start().to_usize()).0)
        .chain(source_map.lines.keys().copied())
        .collect::<BTreeSet<_>>();
    replace_statement_lists(
        py,
        ast,
        &tree,
        &source_map,
        &assertion_lines,
        plans,
        &mut next,
    )?;
    if next != plans.len() {
        return Err(pyo3::exceptions::PyRuntimeError::new_err(
            "assertion rewrite lost an assertion node",
        ));
    }
    let compile = py.import("builtins")?.getattr("compile")?;
    let dont_inherit = py.eval(c"True", None, None)?.clone().into_any();
    let options = kwargs(py, &[("dont_inherit", dont_inherit)])?;
    let code = compile.call((tree.clone(), filename, "exec"), Some(&options))?;
    Ok(code)
}

fn metadata_from_plans(
    source: &str,
    plans: Vec<AssertionPlan>,
    missing_name: &str,
) -> Vec<AssertionMetadata> {
    let line_starts = source_line_starts(source);
    plans
        .into_iter()
        .filter(|plan| plan.instrumented)
        .map(|plan| {
            let (statement_line, statement_column) =
                source_location(&line_starts, plan.statement_range.start().to_usize());
            let (test_line, test_column) =
                source_location(&line_starts, plan.test_range.start().to_usize());
            AssertionMetadata {
                location: Location {
                    line: statement_line,
                    column: u32::try_from(statement_column).unwrap_or(u32::MAX),
                },
                end_line: source_location(&line_starts, plan.statement_range.end().to_usize()).0,
                test_location: Location {
                    line: test_line,
                    column: u32::try_from(test_column).unwrap_or(u32::MAX),
                },
                captures: plan.captures,
                comparison: plan.comparison,
                missing_name: missing_name.to_owned(),
            }
        })
        .collect()
}

fn clear_metadata(filename: &str) -> PyResult<()> {
    registry()
        .write()
        .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("assertion registry poisoned"))?
        .remove(filename);
    Ok(())
}

// Receipt: a 240-character body fits eight short scalar entries in the
// diagnostic width used by the existing terminal snapshots.
const MAX_TEXT: usize = 240;
// Receipt: eight displayed entries across eight nested levels gives 64 nodes.
const MAX_ITEMS: usize = 8;
const MAX_DEPTH: usize = 8;
const MAX_NODES: usize = MAX_ITEMS * MAX_ITEMS;

fn safe_repr(value: &Bound<'_, PyAny>) -> String {
    safe_repr_inner(value, 0, &mut MAX_NODES.clone())
}

fn safe_repr_inner(value: &Bound<'_, PyAny>, depth: usize, budget: &mut usize) -> String {
    if depth > MAX_DEPTH || *budget == 0 {
        return type_fallback(value);
    }
    *budget -= 1;
    if value.is_exact_instance_of::<PyString>() {
        let Ok(string) = value.cast::<PyString>() else {
            return "<unavailable>".to_string();
        };
        let Ok(string) = string.to_str() else {
            return "<unavailable>".to_string();
        };
        let preview = preview_text(string);
        return PyString::new(value.py(), &preview)
            .repr()
            .ok()
            .and_then(|repr| repr.extract::<String>().ok())
            .map_or_else(|| "<unavailable>".to_string(), |repr| clip_quoted(&repr));
    }
    if value.is_exact_instance_of::<PyBytes>() {
        let Ok(bytes) = value.cast::<PyBytes>() else {
            return "<unavailable>".to_string();
        };
        let bytes = bytes.as_bytes();
        let truncated = bytes.len() > MAX_TEXT;
        let preview = &bytes[..bytes.len().min(MAX_TEXT)];
        let rendered = PyBytes::new(value.py(), preview)
            .repr()
            .ok()
            .and_then(|repr| repr.extract::<String>().ok())
            .unwrap_or_else(|| "<unavailable>".to_string());
        return clip(&if truncated {
            format!("{rendered}...")
        } else {
            rendered
        });
    }
    if value.is_none()
        || value.is_exact_instance_of::<PyBool>()
        || value.is_exact_instance_of::<PyFloat>()
        || value.is_exact_instance_of::<pyo3::types::PyComplex>()
    {
        return value
            .repr()
            .ok()
            .and_then(|repr| repr.extract::<String>().ok())
            .map_or_else(|| "<unavailable>".to_string(), |repr| clip(&repr));
    }
    if value.is_exact_instance_of::<PyInt>() {
        let bits = value
            .call_method0("bit_length")
            .ok()
            .and_then(|bits| bits.extract::<usize>().ok());
        if bits.is_some_and(|bits| bits > MAX_TEXT * 8) {
            return format!("<int {} bits>", bits.unwrap_or_default());
        }
        return value
            .repr()
            .ok()
            .and_then(|repr| repr.extract::<String>().ok())
            .map_or_else(|| "<unavailable>".to_string(), |repr| clip(&repr));
    }
    if value.is_exact_instance_of::<PyList>()
        || value.is_exact_instance_of::<PyTuple>()
        || value.is_exact_instance_of::<PyDict>()
        || value.is_exact_instance_of::<pyo3::types::PySet>()
        || value.is_exact_instance_of::<pyo3::types::PyFrozenSet>()
    {
        let identity = value.as_ptr() as usize;
        let _ = identity;
        if let Ok(dictionary) = value.cast::<PyDict>() {
            let mut rendered = Vec::new();
            for (index, (key, item)) in dictionary.iter().enumerate() {
                if index == MAX_ITEMS {
                    break;
                }
                rendered.push(format!(
                    "{}: {}",
                    safe_repr_inner(&key, depth + 1, budget),
                    safe_repr_inner(&item, depth + 1, budget)
                ));
            }
            let suffix = if dictionary.len() > MAX_ITEMS {
                ", ..."
            } else {
                ""
            };
            return clip(&format!("{{{}{suffix}}}", rendered.join(", ")));
        }
        if let Ok(list) = value.cast::<PyList>() {
            let mut rendered = Vec::new();
            for (index, item) in list.iter().enumerate() {
                if index == MAX_ITEMS {
                    break;
                }
                rendered.push(safe_repr_inner(&item, depth + 1, budget));
            }
            let suffix = if list.len() > MAX_ITEMS { ", ..." } else { "" };
            return clip(&format!("[{}{suffix}]", rendered.join(", ")));
        }
        if let Ok(tuple) = value.cast::<PyTuple>() {
            let mut rendered = Vec::new();
            for (index, item) in tuple.iter().enumerate() {
                if index == MAX_ITEMS {
                    break;
                }
                rendered.push(safe_repr_inner(&item, depth + 1, budget));
            }
            let suffix = if tuple.len() > MAX_ITEMS { ", ..." } else { "" };
            let comma = if tuple.len() == 1 { "," } else { "" };
            return clip(&format!("({}{suffix}{comma})", rendered.join(", ")));
        }
    }
    type_fallback(value)
}

fn type_fallback(value: &Bound<'_, PyAny>) -> String {
    let ty = value.get_type();
    let Ok(type_type) = value
        .py()
        .import("builtins")
        .and_then(|builtins| builtins.getattr("type"))
    else {
        return "<object>".to_string();
    };
    let module = type_type
        .call_method1("__getattribute__", (&ty, "__module__"))
        .ok()
        .and_then(|value| value.extract::<String>().ok());
    let qualname = type_type
        .call_method1("__getattribute__", (&ty, "__qualname__"))
        .ok()
        .and_then(|value| value.extract::<String>().ok());
    match (module, qualname) {
        (Some(module), Some(qualname)) => format!("<{module}.{qualname} object>"),
        _ => "<object>".to_string(),
    }
}

fn clip(value: &str) -> String {
    if value.len() <= MAX_TEXT {
        value.to_string()
    } else {
        format!(
            "{}...",
            value.chars().take(MAX_TEXT - 3).collect::<String>()
        )
    }
}

/// Keeps the closing quote visible when an escaped string preview is shortened.
fn clip_quoted(value: &str) -> String {
    if value.chars().count() <= MAX_TEXT {
        return value.to_string();
    }
    let closing = value.chars().last().unwrap_or('\'');
    format!(
        "{}…{closing}",
        value.chars().take(MAX_TEXT - 2).collect::<String>()
    )
}

fn preview_text(value: &str) -> String {
    preview_text_around(value, None)
}

fn preview_text_around(value: &str, focus: Option<usize>) -> String {
    let character_count = value.chars().count();
    if character_count <= MAX_TEXT {
        return value.to_string();
    }
    let window = MAX_TEXT.saturating_sub(2);
    let focus = focus.unwrap_or(0).min(character_count - 1);
    let mut start = focus.saturating_sub(window / 2);
    if start + window > character_count {
        start = character_count - window;
    }
    let body = value.chars().skip(start).take(window).collect::<String>();
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        body,
        if start + window < character_count {
            "…"
        } else {
            ""
        }
    )
}

fn safe_equal(left: &Bound<'_, PyAny>, right: &Bound<'_, PyAny>) -> bool {
    let left_numeric = left.is_exact_instance_of::<PyBool>()
        || left.is_exact_instance_of::<PyInt>()
        || left.is_exact_instance_of::<PyFloat>();
    let right_numeric = right.is_exact_instance_of::<PyBool>()
        || right.is_exact_instance_of::<PyInt>()
        || right.is_exact_instance_of::<PyFloat>();
    if left.get_type().as_ptr() != right.get_type().as_ptr() && !(left_numeric && right_numeric) {
        return false;
    }
    let exact_scalar = left.is_none()
        || left.is_exact_instance_of::<PyBool>()
        || left.is_exact_instance_of::<PyInt>()
        || left.is_exact_instance_of::<PyFloat>()
        || left.is_exact_instance_of::<PyString>()
        || left.is_exact_instance_of::<PyBytes>();
    exact_scalar
        && left
            .rich_compare(right, CompareOp::Eq)
            .and_then(|result| result.is_truthy())
            .unwrap_or(false)
}

fn known_equal(
    left: &Bound<'_, PyAny>,
    right: &Bound<'_, PyAny>,
    depth: usize,
    budget: &mut usize,
) -> Option<bool> {
    // `None` means that proving equality would call user code or exceed the
    // bounded builtin traversal; callers must not treat it as a mismatch.
    if depth > MAX_DEPTH || *budget == 0 {
        return None;
    }
    *budget -= 1;
    let left_numeric = left.is_exact_instance_of::<PyBool>()
        || left.is_exact_instance_of::<PyInt>()
        || left.is_exact_instance_of::<PyFloat>();
    let right_numeric = right.is_exact_instance_of::<PyBool>()
        || right.is_exact_instance_of::<PyInt>()
        || right.is_exact_instance_of::<PyFloat>();
    if left.get_type().as_ptr() != right.get_type().as_ptr() && !(left_numeric && right_numeric) {
        let left_builtin = left.is_none()
            || left.is_exact_instance_of::<PyBool>()
            || left.is_exact_instance_of::<PyInt>()
            || left.is_exact_instance_of::<PyFloat>()
            || left.is_exact_instance_of::<PyString>()
            || left.is_exact_instance_of::<PyBytes>()
            || left.is_exact_instance_of::<PyList>()
            || left.is_exact_instance_of::<PyTuple>()
            || left.is_exact_instance_of::<PyDict>();
        let right_builtin = right.is_none()
            || right.is_exact_instance_of::<PyBool>()
            || right.is_exact_instance_of::<PyInt>()
            || right.is_exact_instance_of::<PyFloat>()
            || right.is_exact_instance_of::<PyString>()
            || right.is_exact_instance_of::<PyBytes>()
            || right.is_exact_instance_of::<PyList>()
            || right.is_exact_instance_of::<PyTuple>()
            || right.is_exact_instance_of::<PyDict>();
        return (left_builtin && right_builtin).then_some(false);
    }
    if left.is_none()
        || left.is_exact_instance_of::<PyBool>()
        || left.is_exact_instance_of::<PyInt>()
        || left.is_exact_instance_of::<PyFloat>()
        || left.is_exact_instance_of::<PyString>()
        || left.is_exact_instance_of::<PyBytes>()
    {
        return Some(safe_equal(left, right));
    }
    if let (Ok(left), Ok(right)) = (left.cast::<PyList>(), right.cast::<PyList>()) {
        if left.len() != right.len() {
            return Some(false);
        }
        for index in 0..left.len() {
            let (Ok(left_item), Ok(right_item)) = (left.get_item(index), right.get_item(index))
            else {
                return None;
            };
            match known_equal(&left_item, &right_item, depth + 1, budget) {
                Some(false) => return Some(false),
                Some(true) => {}
                None => return None,
            }
        }
        return Some(true);
    }
    if let (Ok(left), Ok(right)) = (left.cast::<PyTuple>(), right.cast::<PyTuple>()) {
        if left.len() != right.len() {
            return Some(false);
        }
        for index in 0..left.len() {
            let (Ok(left_item), Ok(right_item)) = (left.get_item(index), right.get_item(index))
            else {
                return None;
            };
            match known_equal(&left_item, &right_item, depth + 1, budget) {
                Some(false) => return Some(false),
                Some(true) => {}
                None => return None,
            }
        }
        return Some(true);
    }
    None
}

fn supported_key(value: &Bound<'_, PyAny>) -> bool {
    value.is_none()
        || value.is_exact_instance_of::<PyBool>()
        || value.is_exact_instance_of::<PyInt>()
        || value.is_exact_instance_of::<PyFloat>()
        || value.is_exact_instance_of::<PyString>()
        || value.is_exact_instance_of::<PyBytes>()
}

fn path_index(path: &str, index: usize) -> String {
    format!("{path}[{index}]")
}

fn path_key(path: &str, key: &Bound<'_, PyAny>) -> String {
    format!("{path}[{}]", safe_repr(key))
}

fn exact_builtin_dict(dict: &Bound<'_, PyDict>) -> bool {
    dict.iter().all(|(key, _)| supported_key(&key))
}

fn same_builtin_container(left: &Bound<'_, PyAny>, right: &Bound<'_, PyAny>) -> bool {
    (left.is_exact_instance_of::<PyDict>() && right.is_exact_instance_of::<PyDict>())
        || (left.is_exact_instance_of::<PyList>() && right.is_exact_instance_of::<PyList>())
        || (left.is_exact_instance_of::<PyTuple>() && right.is_exact_instance_of::<PyTuple>())
}

fn set_contains(set: &Bound<'_, PyAny>, item: &Bound<'_, PyAny>) -> Option<bool> {
    if let Ok(set) = set.cast::<PySet>() {
        return set.contains(item).ok();
    }
    set.cast::<PyFrozenSet>().ok()?.contains(item).ok()
}

fn difference_at(path: &str, left: &str, right: &str) -> Vec<String> {
    let header = if path.is_empty() {
        "Difference:".to_string()
    } else {
        format!("Difference at {path}:")
    };
    vec![
        header,
        format!("  left: {left}"),
        format!("  right: {right}"),
    ]
}

fn different_lengths(path: &str, left: usize, right: usize) -> Vec<String> {
    if path.is_empty() {
        vec![
            "Different lengths:".to_string(),
            format!("  left: {left}"),
            format!("  right: {right}"),
        ]
    } else {
        vec![
            format!("Difference at {path}:"),
            "  Different lengths:".to_string(),
            format!("    left: {left}"),
            format!("    right: {right}"),
        ]
    }
}

fn focused_diff_inner(
    left: &Bound<'_, PyAny>,
    right: &Bound<'_, PyAny>,
    path: &str,
    depth: usize,
) -> Option<Vec<String>> {
    if depth > MAX_DEPTH {
        return None;
    }
    if let (Ok(left), Ok(right)) = (left.cast::<PyDict>(), right.cast::<PyDict>()) {
        // Looking up a key can call arbitrary equality methods.  Only use the
        // focused mapping view when every key on both sides is an exact scalar
        // whose hash/equality is builtin and bounded.
        if !exact_builtin_dict(left) || !exact_builtin_dict(right) {
            return None;
        }
        for (left_key, left_value) in left.iter() {
            let key_path = path_key(path, &left_key);
            let Ok(Some(right_value)) = right.get_item(&left_key) else {
                return Some(difference_at(
                    &key_path,
                    &safe_repr(&left_value),
                    "<missing>",
                ));
            };
            let nested_container = same_builtin_container(&left_value, &right_value);
            let known = known_equal(&left_value, &right_value, 0, &mut MAX_NODES.clone());
            if known == Some(false) || nested_container {
                if let Some(diff) =
                    focused_diff_inner(&left_value, &right_value, &key_path, depth + 1)
                {
                    return Some(diff);
                }
                if known == Some(false) {
                    return Some(difference_at(
                        &key_path,
                        &safe_repr(&left_value),
                        &safe_repr(&right_value),
                    ));
                }
            }
        }
        for (right_key, right_value) in right.iter() {
            if left.get_item(&right_key).ok().flatten().is_some() {
                continue;
            }
            let key_path = path_key(path, &right_key);
            return Some(difference_at(
                &key_path,
                "<missing>",
                &safe_repr(&right_value),
            ));
        }
        if left.len() != right.len() {
            return Some(different_lengths(path, left.len(), right.len()));
        }
    }
    if let (Ok(left), Ok(right)) = (left.cast::<PyList>(), right.cast::<PyList>()) {
        if left.len() != right.len() {
            return Some(different_lengths(path, left.len(), right.len()));
        }
        for index in 0..left.len() {
            let (Ok(left_value), Ok(right_value)) = (left.get_item(index), right.get_item(index))
            else {
                continue;
            };
            let nested_container = same_builtin_container(&left_value, &right_value);
            let known = known_equal(&left_value, &right_value, 0, &mut MAX_NODES.clone());
            if known == Some(false) || nested_container {
                let item_path = path_index(path, index);
                if let Some(diff) =
                    focused_diff_inner(&left_value, &right_value, &item_path, depth + 1)
                {
                    return Some(diff);
                }
                if known == Some(false) {
                    return Some(difference_at(
                        &item_path,
                        &safe_repr(&left_value),
                        &safe_repr(&right_value),
                    ));
                }
            }
        }
    }
    if let (Ok(left), Ok(right)) = (left.cast::<PyTuple>(), right.cast::<PyTuple>()) {
        if left.len() != right.len() {
            return Some(different_lengths(path, left.len(), right.len()));
        }
        for index in 0..left.len() {
            let (Ok(left_value), Ok(right_value)) = (left.get_item(index), right.get_item(index))
            else {
                continue;
            };
            let nested_container = same_builtin_container(&left_value, &right_value);
            let known = known_equal(&left_value, &right_value, 0, &mut MAX_NODES.clone());
            if known == Some(false) || nested_container {
                let item_path = path_index(path, index);
                if let Some(diff) =
                    focused_diff_inner(&left_value, &right_value, &item_path, depth + 1)
                {
                    return Some(diff);
                }
                if known == Some(false) {
                    return Some(difference_at(
                        &item_path,
                        &safe_repr(&left_value),
                        &safe_repr(&right_value),
                    ));
                }
            }
        }
    }
    if left.is_exact_instance_of::<PyBytes>() && right.is_exact_instance_of::<PyBytes>() {
        let (Ok(left), Ok(right)) = (left.cast::<PyBytes>(), right.cast::<PyBytes>()) else {
            return None;
        };
        let left = left.as_bytes();
        let right = right.as_bytes();
        if left.len() != right.len() {
            return Some(different_lengths(path, left.len(), right.len()));
        }
        if let Some((index, (left, right))) = left
            .iter()
            .zip(right)
            .enumerate()
            .find(|(_, (left, right))| left != right)
        {
            return Some(difference_at(
                &path_index(path, index),
                &left.to_string(),
                &right.to_string(),
            ));
        }
    }
    if (left.is_exact_instance_of::<PySet>() && right.is_exact_instance_of::<PySet>())
        || (left.is_exact_instance_of::<PyFrozenSet>()
            && right.is_exact_instance_of::<PyFrozenSet>())
    {
        if left
            .try_iter()
            .ok()?
            .flatten()
            .any(|item| !supported_key(&item))
            || right
                .try_iter()
                .ok()?
                .flatten()
                .any(|item| !supported_key(&item))
        {
            return None;
        }
        let mut differences = vec!["Set difference:".to_string()];
        for (set, other, label) in [(left, right, "left"), (right, left, "right")] {
            let mut displayed = 0;
            let mut omitted = 0;
            for item in set.try_iter().ok()?.flatten() {
                if !set_contains(other, &item)? {
                    if displayed < MAX_ITEMS {
                        differences.push(format!("  {label} only: {}", safe_repr(&item)));
                        displayed += 1;
                    } else {
                        omitted += 1;
                    }
                }
            }
            if omitted > 0 {
                differences.push(format!("  {label} only: ({omitted} more omitted)"));
            }
        }
        if differences.len() > 1 {
            return Some(differences);
        }
    }
    if left.is_exact_instance_of::<PyString>() && right.is_exact_instance_of::<PyString>() {
        let left_object = left.cast::<PyString>().ok()?;
        let right_object = right.cast::<PyString>().ok()?;
        let py = left_object.py();
        let left = left_object.to_str().ok()?;
        let right = right_object.to_str().ok()?;
        if left == right {
            return None;
        }
        let focus = left
            .chars()
            .zip(right.chars())
            .position(|(left, right)| left != right)
            .unwrap_or_else(|| left.chars().count().min(right.chars().count()));
        if !left.contains('\n') && !right.contains('\n') {
            let header = if path.is_empty() {
                "String difference:".to_string()
            } else {
                format!("Difference at {path}:")
            };
            return Some(vec![
                header,
                format!(
                    "  left: {}",
                    safe_repr(&PyString::new(py, &preview_text_around(left, Some(focus))))
                ),
                format!(
                    "  right: {}",
                    safe_repr(&PyString::new(py, &preview_text_around(right, Some(focus))))
                ),
            ]);
        }
        let header = if path.is_empty() {
            "String difference (- left, + right):".to_string()
        } else {
            format!("Difference at {path}:\nString difference (- left, + right):")
        };
        return Some(vec![
            header,
            karva_snapshot::diff::format_diff(
                &preview_text_around(left, Some(focus)),
                &preview_text_around(right, Some(focus)),
            ),
        ]);
    }
    None
}

fn focused_diff(left: &Bound<'_, PyAny>, right: &Bound<'_, PyAny>) -> Vec<String> {
    focused_diff_inner(left, right, "", 0).unwrap_or_default()
}

fn render_explanation(
    metadata: &AssertionMetadata,
    missing: Option<&Bound<'_, PyAny>>,
    records: Vec<EvaluatedCapture<'_>>,
) -> String {
    let mut values: Vec<(String, String, Bound<'_, PyAny>, bool)> = Vec::new();
    for record in records {
        if record.optional
            && missing.is_some_and(|missing| missing.as_ptr() == record.value.as_ptr())
        {
            continue;
        }
        let rendered = safe_repr(&record.value);
        let label = record.label;
        if metadata.comparison == ComparisonKind::Other {
            if let Some(index) = values
                .iter()
                .position(|(existing, _, _, _)| existing == &label)
            {
                values.remove(index);
            }
        }
        values.push((label, rendered, record.value, record.literal));
    }
    if metadata.comparison == ComparisonKind::Equality && values.len() == 2 {
        let diff = focused_diff(&values[0].2, &values[1].2);
        if !diff.is_empty() {
            return diff.join("\n");
        }
    }
    values.retain(|(_, _, _, literal)| !literal);
    if values.is_empty() {
        return String::new();
    }
    let mut lines = vec!["Evaluated values:".to_string()];
    for (index, (label, rendered, _, _)) in values.iter().enumerate() {
        let label = if values.len() == 2
            && matches!(
                metadata.comparison,
                ComparisonKind::Equality | ComparisonKind::Comparison | ComparisonKind::Identity
            ) {
            format!("{} ({label})", if index == 0 { "left" } else { "right" })
        } else {
            label.clone()
        };
        lines.push(format!("  {label} = {rendered}"));
    }
    if metadata.comparison == ComparisonKind::Identity
        && values.len() == 2
        && values[0].1 == values[1].1
        && values[0].2.as_ptr() != values[1].2.as_ptr()
    {
        return format!("Distinct objects (both rendered as {})", values[0].1);
    }
    lines.join("\n")
}

#[pyclass]
struct AssertionFinder {
    roots: Arc<RwLock<BTreeSet<PathBuf>>>,
}

#[pyclass]
struct AssertionLoader {
    fullname: String,
    filename: String,
    original: Py<PyAny>,
}

#[pymethods]
impl AssertionFinder {
    #[pyo3(signature=(fullname, path=None, target=None))]
    fn find_spec(
        &self,
        py: Python<'_>,
        fullname: &str,
        path: Option<Bound<'_, PyAny>>,
        target: Option<Bound<'_, PyAny>>,
    ) -> PyResult<Option<Py<PyAny>>> {
        let machinery = py.import("importlib.machinery")?;
        let finder = machinery.getattr("PathFinder")?;
        let path = path.unwrap_or_else(|| py.None().into_bound(py));
        let target = target.unwrap_or_else(|| py.None().into_bound(py));
        let spec = finder.call_method1("find_spec", (fullname, path, target))?;
        if spec.is_none() {
            return Ok(None);
        }
        let origin = spec.getattr("origin")?.extract::<Option<String>>()?;
        let Some(origin) = origin else {
            return Ok(None);
        };
        let origin = PathBuf::from(origin);
        if origin.extension().and_then(|extension| extension.to_str()) != Some("py")
            || origin.components().any(|component| {
                matches!(
                    component.as_os_str().to_str(),
                    Some(".venv" | "venv" | ".tox" | ".nox" | "site-packages")
                )
            })
        {
            return Ok(None);
        }
        let roots = self
            .roots
            .read()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("assertion roots poisoned"))?
            .clone();
        let origin = if roots.iter().any(|root| origin.starts_with(root)) {
            canonical_filename(&origin)
        } else {
            let canonical = canonical_filename(&origin);
            if !roots.iter().any(|root| canonical.starts_with(root)) {
                return Ok(None);
            }
            canonical
        };
        if !roots.iter().any(|root| origin.starts_with(root)) {
            return Ok(None);
        }
        let loader = spec.getattr("loader")?;
        let source_loader = machinery.getattr("SourceFileLoader")?;
        let isinstance = py.import("builtins")?.getattr("isinstance")?;
        if !isinstance
            .call1((loader.clone(), source_loader))?
            .is_truthy()?
        {
            return Ok(None);
        }
        let loader = Py::new(
            py,
            AssertionLoader {
                fullname: fullname.to_string(),
                filename: origin.to_string_lossy().into_owned(),
                original: loader.unbind(),
            },
        )?;
        spec.setattr("loader", loader.clone_ref(py))?;
        Ok(Some(spec.unbind()))
    }
}

#[pymethods]
impl AssertionLoader {
    fn create_module(&self, py: Python<'_>, spec: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        self.original
            .bind(py)
            .call_method1("create_module", (spec,))
            .map(Bound::unbind)
    }

    fn exec_module(&self, py: Python<'_>, module: &Bound<'_, PyAny>) -> PyResult<()> {
        clear_metadata(&self.filename)?;
        let optimize = py
            .import("sys")?
            .getattr("flags")?
            .getattr("optimize")?
            .extract::<u8>()?;
        if optimize > 0 {
            self.original
                .bind(py)
                .call_method1("exec_module", (module,))?;
            return Ok(());
        }
        let (source, source_bytes) = if let Ok(raw) = self
            .original
            .bind(py)
            .call_method1("get_data", (&self.filename,))
        {
            let bytes = raw.cast::<PyBytes>()?.clone();
            let source = py
                .import("importlib.util")?
                .call_method1("decode_source", (&bytes,))?
                .extract::<String>()?;
            (source, bytes)
        } else {
            let source = self
                .original
                .bind(py)
                .call_method1("get_source", (&self.fullname,))?;
            if source.is_none() {
                self.original
                    .bind(py)
                    .call_method1("exec_module", (module,))?;
                return Ok(());
            }
            let source = source.extract::<String>()?;
            let bytes = PyBytes::new(py, source.as_bytes()).clone();
            (source, bytes)
        };
        if !source.contains("assert") {
            self.original
                .bind(py)
                .call_method1("exec_module", (module,))?;
            return Ok(());
        }
        let python_version = current_python_version(py);
        let missing_name = missing_name_for_source(&source);
        let dict = module.getattr("__dict__")?.cast_into::<PyDict>()?;
        let code = if let Some(code) = cache::read(
            py,
            self.original.bind(py),
            &self.fullname,
            &self.filename,
            &source_bytes,
        ) {
            registry()
                .write()
                .map_err(|_| {
                    pyo3::exceptions::PyRuntimeError::new_err("assertion registry poisoned")
                })?
                .insert(
                    self.filename.clone(),
                    RegistryEntry::Deferred {
                        source,
                        python_version,
                        missing_name: missing_name.clone(),
                    },
                );
            code
        } else {
            let Some(CollectedPlans { plans }) = collect_plans(&source, python_version) else {
                self.original
                    .bind(py)
                    .call_method1("exec_module", (module,))?;
                return Ok(());
            };
            if plans.iter().all(|plan| !plan.instrumented) {
                self.original
                    .bind(py)
                    .call_method1("exec_module", (module,))?;
                return Ok(());
            }
            let ast = py.import("ast")?;
            let code =
                compile_assertions(py, &ast, &source, &self.filename, &plans, &missing_name)?;
            cache::write(
                py,
                self.original.bind(py),
                &self.filename,
                &source_bytes,
                &code,
            );
            let metadata = metadata_from_plans(&source, plans, &missing_name);
            registry()
                .write()
                .map_err(|_| {
                    pyo3::exceptions::PyRuntimeError::new_err("assertion registry poisoned")
                })?
                .insert(self.filename.clone(), RegistryEntry::Ready(metadata));
            code
        };
        dict.set_item(&missing_name, py.eval(c"object()", None, None)?)?;
        py.import("builtins")?
            .getattr("exec")?
            .call((code, dict.clone(), dict), None)?;
        Ok(())
    }

    fn get_filename(&self, py: Python<'_>, fullname: &str) -> PyResult<Py<PyAny>> {
        self.original
            .bind(py)
            .call_method1("get_filename", (fullname,))
            .map(Bound::unbind)
    }

    fn get_source(&self, py: Python<'_>, fullname: &str) -> PyResult<Py<PyAny>> {
        self.original
            .bind(py)
            .call_method1("get_source", (fullname,))
            .map(Bound::unbind)
    }

    fn get_code(&self, py: Python<'_>, fullname: &str) -> PyResult<Py<PyAny>> {
        self.original
            .bind(py)
            .call_method1("get_code", (fullname,))
            .map(Bound::unbind)
    }

    fn is_package(&self, py: Python<'_>, fullname: &str) -> PyResult<Py<PyAny>> {
        self.original
            .bind(py)
            .call_method1("is_package", (fullname,))
            .map(Bound::unbind)
    }

    fn get_resource_reader(&self, py: Python<'_>, fullname: &str) -> PyResult<Py<PyAny>> {
        self.original
            .bind(py)
            .call_method1("get_resource_reader", (fullname,))
            .map(Bound::unbind)
    }

    fn get_data(&self, py: Python<'_>, path: &str) -> PyResult<Py<PyAny>> {
        self.original
            .bind(py)
            .call_method1("get_data", (path,))
            .map(Bound::unbind)
    }
}

fn current_python_version(py: Python<'_>) -> PythonVersion {
    let version = py.version_info();
    PythonVersion::from((version.major, version.minor))
}

/// Keeps filesystem identity while removing Windows extended-path prefixes
/// from filenames handed to Python's compiler and traceback renderer.
fn canonical_filename(path: &std::path::Path) -> PathBuf {
    std::fs::canonicalize(path)
        .map(|path| dunce::simplified(&path).to_path_buf())
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Installs the first-party assertion finder for a project root.
pub fn install(py: Python<'_>, root: &Utf8Path) -> PyResult<()> {
    let roots = if let Some(existing) = py
        .import("sys")?
        .getattr("meta_path")?
        .cast::<PyList>()?
        .iter()
        .find(PyAnyMethods::is_instance_of::<AssertionFinder>)
    {
        existing.cast::<AssertionFinder>()?.borrow().roots.clone()
    } else {
        let roots = Arc::new(RwLock::new(BTreeSet::new()));
        let finder = Py::new(
            py,
            AssertionFinder {
                roots: roots.clone(),
            },
        )?;
        py.import("sys")?
            .getattr("meta_path")?
            .cast::<PyList>()?
            .insert(0, finder)?;
        roots
    };
    roots
        .write()
        .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("assertion roots poisoned"))?
        .insert(canonical_filename(root.as_std_path()));
    Ok(())
}

fn instruction_column(frame: &Bound<'_, PyAny>, traceback: &Bound<'_, PyAny>) -> Option<u32> {
    let lasti = traceback
        .getattr("tb_lasti")
        .ok()?
        .extract::<isize>()
        .ok()?;
    if lasti < 0 {
        return None;
    }
    let positions = frame
        .getattr("f_code")
        .ok()?
        .getattr("co_positions")
        .ok()?
        .call0()
        .ok()?
        .cast_into::<pyo3::types::PyIterator>()
        .ok()?;
    let instruction = usize::try_from(lasti).ok()?.checked_div(2)?;
    positions.enumerate().find_map(|(index, position)| {
        if index != instruction {
            return None;
        }
        let position = position.ok()?.cast_into::<PyTuple>().ok()?;
        position.get_item(2).ok()?.extract::<u32>().ok()
    })
}

/// Materializes a cached module's metadata once, without invoking Python while
/// the registry is locked.
fn realize_deferred(filename: &str) -> Option<()> {
    let mut registry = registry().write().ok()?;
    let (source, python_version, missing_name) = match registry.get(filename)? {
        RegistryEntry::Ready(_) => return Some(()),
        RegistryEntry::Deferred {
            source,
            python_version,
            missing_name,
        } => (source.clone(), *python_version, missing_name.clone()),
    };
    let metadata = collect_plans(&source, python_version)
        .map(|collected| metadata_from_plans(&source, collected.plans, &missing_name))
        .unwrap_or_default();
    registry.insert(filename.to_owned(), RegistryEntry::Ready(metadata));
    Some(())
}

/// Explains a failed assertion without reevaluating expressions or invoking
/// user representations. Cached metadata is reconstructed only on failure.
pub fn explain(py: Python<'_>, error: &PyErr) -> Option<String> {
    let mut traceback = error.traceback(py)?.into_any();
    loop {
        let next = traceback.getattr("tb_next").ok()?;
        if next.is_none() {
            break;
        }
        traceback = next;
    }
    let frame = traceback.getattr("tb_frame").ok()?;
    let line = traceback.getattr("tb_lineno").ok()?.extract::<u32>().ok()?;
    let filename = frame
        .getattr("f_code")
        .ok()?
        .getattr("co_filename")
        .ok()?
        .extract::<String>()
        .ok()?;
    let registry_filename = {
        let registry = registry().read().ok()?;
        if registry.contains_key(&filename) {
            filename
        } else {
            let canonical = canonical_filename(std::path::Path::new(&filename));
            registry
                .contains_key(canonical.to_string_lossy().as_ref())
                .then(|| canonical.to_string_lossy().into_owned())?
        }
    };
    realize_deferred(&registry_filename)?;
    let registry = registry().read().ok()?;
    let RegistryEntry::Ready(metadata) = registry.get(&registry_filename)? else {
        return None;
    };
    let column = instruction_column(&frame, &traceback);
    let candidates = metadata
        .iter()
        .filter(|metadata| metadata.location.line <= line && line <= metadata.end_line)
        .collect::<Vec<_>>();
    let locals = frame.getattr("f_locals").ok()?;
    let matching = if let Some(column) = column {
        candidates
            .iter()
            .copied()
            .filter(|metadata| metadata.test_location.column == column)
            .collect::<Vec<_>>()
    } else if candidates.len() == 1 {
        candidates
    } else {
        candidates
            .into_iter()
            .filter(|metadata| {
                metadata
                    .captures
                    .iter()
                    .any(|capture| locals.get_item(&capture.name).is_ok())
            })
            .collect::<Vec<_>>()
    };
    let metadata = (matching.len() == 1).then(|| matching[0])?.clone();
    drop(registry);
    let missing = frame
        .getattr("f_globals")
        .ok()
        .and_then(|globals| globals.get_item(&metadata.missing_name).ok());
    let records: Vec<EvaluatedCapture<'_>> = metadata
        .captures
        .iter()
        .filter_map(|capture| {
            let value = locals.get_item(&capture.name).ok()?;
            Some(EvaluatedCapture {
                label: capture.label.clone(),
                value,
                literal: capture.literal,
                optional: capture.optional,
            })
        })
        .collect();
    Some(render_explanation(&metadata, missing.as_ref(), records))
}
