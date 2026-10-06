//! Rust-owned assertion instrumentation for first-party Python modules.
//!
//! Ruff identifies assertion and operand ranges. Rust applies source edits for
//! captures, then uses CPython's standard AST only for statement cleanup and
//! compilation. This keeps Python's compiler semantics while avoiding a second
//! expression-tree implementation in Rust.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use camino::Utf8Path;
use pyo3::class::basic::CompareOp;
use pyo3::prelude::*;
use pyo3::types::{
    PyAnyMethods, PyBool, PyBytes, PyDict, PyDictMethods, PyFloat, PyFrozenSet, PyInt, PyList,
    PyListMethods, PySet, PyString, PyStringMethods, PyTuple, PyTupleMethods,
};
use ruff_python_ast::visitor::{Visitor, walk_stmt};
use ruff_python_ast::{CmpOp, Expr, PythonVersion, Stmt};
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
use ruff_text_size::{Ranged, TextRange};

const VALUE_PREFIX: &str = "_karva_value_";
const MISSING_PREFIX: &str = "_karva_missing";

// Entries live for the worker process and are keyed by canonical filename;
// imports replace a file's metadata atomically before its code can run.
static REGISTRY: OnceLock<RwLock<HashMap<String, Vec<AssertionMetadata>>>> = OnceLock::new();

fn registry() -> &'static RwLock<HashMap<String, Vec<AssertionMetadata>>> {
    REGISTRY.get_or_init(|| RwLock::new(HashMap::new()))
}

#[derive(Clone, Debug)]
struct Location {
    /// CPython's one-based source line and UTF-8 byte column.
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
    Comparison,
    Identity,
}

#[derive(Clone, Debug)]
struct AssertionMetadata {
    /// The compiled `Assert` location; `test_location` selects same-line failures.
    location: Location,
    test_location: Location,
    source: String,
    captures: Vec<CaptureMetadata>,
    comparison: ComparisonKind,
    missing_name: String,
}

#[derive(Debug)]
struct AssertionPlan {
    test_range: TextRange,
    source: String,
    captures: Vec<CaptureMetadata>,
    comparison: ComparisonKind,
}

struct EvaluatedCapture<'py> {
    label: String,
    value: Bound<'py, PyAny>,
    literal: bool,
    optional: bool,
}

struct AssertionCollector<'a> {
    source: &'a str,
    names: BTreeSet<String>,
    plans: Vec<AssertionPlan>,
    value_index: usize,
}

impl<'a> AssertionCollector<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            names: BTreeSet::new(),
            plans: Vec::new(),
            value_index: 0,
        }
    }

    fn collect_names(&mut self) {
        for token in self
            .source
            .split(|character: char| !(character == '_' || character.is_ascii_alphanumeric()))
        {
            if token
                .chars()
                .next()
                .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
            {
                self.names.insert(token.to_string());
            }
        }
    }

    fn next_name(&mut self) -> String {
        loop {
            let name = format!("{VALUE_PREFIX}{}", self.value_index);
            self.value_index += 1;
            if self.names.insert(name.clone()) {
                return name;
            }
        }
    }

    fn collect_assert(&mut self, assertion: &ruff_python_ast::StmtAssert) {
        let source = self.source_slice(assertion.test.range()).trim().to_string();
        let comparison = match assertion.test.as_ref() {
            Expr::Compare(compare)
                if compare
                    .ops
                    .iter()
                    .any(|op| matches!(op, CmpOp::Is | CmpOp::IsNot)) =>
            {
                ComparisonKind::Identity
            }
            Expr::Compare(_) => ComparisonKind::Comparison,
            _ => ComparisonKind::Other,
        };
        let mut captures = Vec::new();
        self.collect_expr(&assertion.test, false, &mut captures);
        self.plans.push(AssertionPlan {
            test_range: assertion.test.range(),
            source,
            captures,
            comparison,
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
                self.capture(compare.left.as_ref(), may_skip, captures);
                for (index, comparator) in compare.comparators.iter().enumerate() {
                    self.capture(comparator, may_skip || index > 0, captures);
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

fn collect_plans(source: &str, python_version: PythonVersion) -> Option<Vec<AssertionPlan>> {
    let options = ParseOptions::from(Mode::Module).with_target_version(python_version);
    let parsed = parse_unchecked(source, options).try_into_module()?;
    let mut collector = AssertionCollector::new(source);
    collector.collect_names();
    collector.visit_body(parsed.suite());
    Some(collector.plans)
}

struct Edit {
    range: TextRange,
    replacement: String,
}

fn rewrite_source(source: &str, plans: &[AssertionPlan], missing_name: &str) -> String {
    let mut edits = Vec::new();
    for plan in plans {
        for capture in &plan.captures {
            let original =
                &source[capture.range.start().to_usize()..capture.range.end().to_usize()];
            edits.push(Edit {
                range: capture.range,
                replacement: format!("({} := ({}))", capture.name, original),
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
            });
        }
    }
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.range.start()));
    let mut rewritten = source.to_string();
    for edit in edits {
        let start = edit.range.start().to_usize();
        let end = edit.range.end().to_usize();
        rewritten.replace_range(start..end, &edit.replacement);
    }
    rewritten
}

fn line_location(node: &Bound<'_, PyAny>) -> PyResult<Location> {
    Ok(Location {
        line: node.getattr("lineno")?.extract()?,
        column: node.getattr("col_offset")?.extract()?,
    })
}

fn assertion_locations(node: &Bound<'_, PyAny>) -> PyResult<(Location, Location)> {
    Ok((line_location(node)?, line_location(&node.getattr("test")?)?))
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
        .map(|capture| make_name(py, ast, &capture.name, "Del"))
        .collect::<PyResult<Vec<_>>>()?;
    let delete = construct(
        py,
        ast,
        "Delete",
        &[("targets", targets.into_pyobject(py)?.into_any())],
    )?;
    let wrapper = construct(
        py,
        ast,
        "If",
        &[
            ("test", make_name(py, ast, "__debug__", "Load")?),
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
    node: Bound<'py, PyAny>,
    plans: &[AssertionPlan],
    next: &mut usize,
) -> PyResult<()> {
    let fields = ast
        .getattr("iter_fields")?
        .call1((node.clone(),))?
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
                    list.set_item(index, transform_assert(py, ast, item, plan)?)?;
                    *next += 1;
                } else if item.hasattr("_fields")? {
                    replace_statement_lists(py, ast, item, plans, next)?;
                }
            }
        } else if value.hasattr("_fields")? {
            replace_statement_lists(py, ast, value, plans, next)?;
        }
    }
    Ok(())
}

fn compile_and_exec<'py>(
    py: Python<'py>,
    ast: &Bound<'py, PyModule>,
    source: &str,
    filename: &str,
    module_dict: &Bound<'py, PyDict>,
    plans: Vec<AssertionPlan>,
) -> PyResult<()> {
    let mut names = BTreeSet::new();
    for token in
        source.split(|character: char| !(character == '_' || character.is_ascii_alphanumeric()))
    {
        if token
            .chars()
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        {
            names.insert(token.to_string());
        }
    }
    let missing_name = (0..)
        .map(|suffix| {
            if suffix == 0 {
                MISSING_PREFIX.to_string()
            } else {
                format!("{MISSING_PREFIX}_{suffix}")
            }
        })
        .find(|candidate| !names.contains(candidate))
        .unwrap_or_else(|| format!("{MISSING_PREFIX}_fallback"));
    let rewritten = rewrite_source(source, &plans, &missing_name);
    module_dict.set_item(&missing_name, py.eval(c"object()", None, None)?)?;
    let tree = ast.getattr("parse")?.call((rewritten, filename), None)?;
    let mut next = 0;
    replace_statement_lists(py, ast, tree.clone(), &plans, &mut next)?;
    if next != plans.len() {
        return Err(pyo3::exceptions::PyRuntimeError::new_err(
            "assertion rewrite lost an assertion node",
        ));
    }
    ast.getattr("fix_missing_locations")?
        .call1((tree.clone(),))?;
    let compile = py.import("builtins")?.getattr("compile")?;
    let dont_inherit = py.eval(c"True", None, None)?.to_owned().into_any();
    let options = kwargs(py, &[("dont_inherit", dont_inherit)])?;
    let code = compile.call((tree.clone(), filename, "exec"), Some(&options))?;
    let mut locations = ast
        .getattr("walk")?
        .call1((tree.clone(),))?
        .cast_into::<pyo3::types::PyIterator>()?
        .filter_map(|node| node.ok())
        .filter(|node| node_kind(node).is_ok_and(|kind| kind == "Assert"))
        .map(|node| assertion_locations(&node))
        .collect::<PyResult<Vec<_>>>()?;
    locations.sort_by_key(|(location, _)| (location.line, location.column));
    if locations.len() != plans.len() {
        return Err(pyo3::exceptions::PyRuntimeError::new_err(
            "assertion rewrite lost an assertion location",
        ));
    }
    let metadata = plans
        .into_iter()
        .zip(locations)
        .map(|(plan, (location, test_location))| AssertionMetadata {
            location,
            test_location,
            source: plan.source,
            captures: plan.captures,
            comparison: plan.comparison,
            missing_name: missing_name.clone(),
        })
        .collect();
    registry()
        .write()
        .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("assertion registry poisoned"))?
        .insert(filename.to_string(), metadata);
    py.import("builtins")?
        .getattr("exec")?
        .call((code, module_dict, module_dict), None)?;
    Ok(())
}

const MAX_TEXT: usize = 240;
const MAX_ITEMS: usize = 8;
const MAX_DEPTH: usize = 8;
const MAX_NODES: usize = 64;

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
            .map_or_else(|| "<unavailable>".to_string(), |repr| clip(&repr));
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
            format!("{}...", rendered)
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
            let suffix = (dictionary.len() > MAX_ITEMS)
                .then_some(", ...")
                .unwrap_or("");
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
            let suffix = (list.len() > MAX_ITEMS).then_some(", ...").unwrap_or("");
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
            let suffix = (tuple.len() > MAX_ITEMS).then_some(", ...").unwrap_or("");
            let comma = (tuple.len() == 1).then_some(",").unwrap_or("");
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

fn preview_text(value: &str) -> String {
    let mut characters = value.chars();
    let prefix = characters.by_ref().take(MAX_TEXT).collect::<String>();
    if characters.next().is_none() {
        return prefix;
    }
    let suffix = value
        .chars()
        .rev()
        .take(MAX_TEXT / 2)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!(
        "{}…{}",
        prefix.chars().take(MAX_TEXT / 2).collect::<String>(),
        suffix
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
        return Some(false);
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

fn focused_diff(left: &Bound<'_, PyAny>, right: &Bound<'_, PyAny>) -> Vec<String> {
    if let (Ok(left), Ok(right)) = (left.cast::<PyDict>(), right.cast::<PyDict>()) {
        let mut result = Vec::new();
        for (left_key, left_value) in left.iter().take(MAX_NODES) {
            let supported_key = left_key.is_none()
                || left_key.is_exact_instance_of::<PyBool>()
                || left_key.is_exact_instance_of::<PyInt>()
                || left_key.is_exact_instance_of::<PyFloat>()
                || left_key.is_exact_instance_of::<PyString>()
                || left_key.is_exact_instance_of::<PyBytes>();
            if !supported_key {
                continue;
            }
            let left_key_repr = safe_repr(&left_key);
            let Ok(Some(right_value)) = right.get_item(&left_key) else {
                continue;
            };
            if known_equal(&left_value, &right_value, 0, &mut MAX_NODES.clone()) == Some(false) {
                result.push(format!(
                    "actual[{left_key_repr}]: {}",
                    safe_repr(&left_value)
                ));
                result.push(format!(
                    "expected[{left_key_repr}]: {}",
                    safe_repr(&right_value)
                ));
                return result;
            }
        }
    }
    if let (Ok(left), Ok(right)) = (left.cast::<PyList>(), right.cast::<PyList>()) {
        // This scans the exact built-in sequence without materializing it; the
        // eight-item cap applies only to rendered containers.
        for index in 0..left.len().min(right.len()) {
            let Ok(left_value) = left.get_item(index) else {
                continue;
            };
            let Ok(right_value) = right.get_item(index) else {
                continue;
            };
            if known_equal(&left_value, &right_value, 0, &mut MAX_NODES.clone()) == Some(false) {
                return vec![
                    format!("actual[{index}]: {}", safe_repr(&left_value)),
                    format!("expected[{index}]: {}", safe_repr(&right_value)),
                ];
            }
        }
    }
    if (left.is_exact_instance_of::<PyBytes>() && right.is_exact_instance_of::<PyBytes>())
        || (left.is_exact_instance_of::<PySet>() && right.is_exact_instance_of::<PySet>())
        || (left.is_exact_instance_of::<PyFrozenSet>()
            && right.is_exact_instance_of::<PyFrozenSet>())
    {
        if safe_repr(left) != safe_repr(right) {
            return vec![
                format!("actual: {}", safe_repr(left)),
                format!("expected: {}", safe_repr(right)),
            ];
        }
    }
    if left.is_exact_instance_of::<PyString>() && right.is_exact_instance_of::<PyString>() {
        let Ok(left) = left.cast::<PyString>() else {
            return Vec::new();
        };
        let Ok(left) = left.to_str() else {
            return Vec::new();
        };
        let Ok(right) = right.cast::<PyString>() else {
            return Vec::new();
        };
        let Ok(right) = right.to_str() else {
            return Vec::new();
        };
        if left != right {
            return vec![
                "string diff:".to_string(),
                karva_snapshot::diff::format_diff(&preview_text(left), &preview_text(right)),
            ];
        }
    }
    Vec::new()
}

fn render_explanation(
    metadata: &AssertionMetadata,
    missing: Option<&Bound<'_, PyAny>>,
    records: Vec<EvaluatedCapture<'_>>,
) -> String {
    let mut lines = vec![format!("assert {}", clip(&metadata.source))];
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
    if metadata.comparison != ComparisonKind::Other && values.len() == 2 {
        let diff = focused_diff(&values[0].2, &values[1].2);
        if !diff.is_empty() {
            lines.push(String::new());
            lines.push("Differing values:".to_string());
            lines.extend(diff.into_iter().map(|line| format!("  {line}")));
            return lines.join("\n");
        }
    }
    values.retain(|(_, _, _, literal)| !literal);
    if !values.is_empty() {
        lines.push(String::new());
        lines.push("Differing values:".to_string());
        if metadata.comparison == ComparisonKind::Identity
            && values.len() == 2
            && values[0].1 == values[1].1
            && values[0].2.as_ptr() != values[1].2.as_ptr()
        {
            lines.extend(values.iter().map(|(label, rendered, _, _)| {
                format!("  {label}: {rendered} (distinct objects)")
            }));
        } else {
            lines.extend(
                values
                    .iter()
                    .map(|(label, rendered, _, _)| format!("  {label}: {rendered}")),
            );
        }
    }
    lines.join("\n")
}

#[pyclass]
struct AssertionFinder {
    roots: Arc<Mutex<BTreeSet<PathBuf>>>,
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
        let origin = std::fs::canonicalize(&origin).unwrap_or_else(|_| PathBuf::from(&origin));
        let in_root = self
            .roots
            .lock()
            .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("assertion roots poisoned"))?
            .iter()
            .any(|root| origin.starts_with(root));
        if origin.extension().and_then(|extension| extension.to_str()) != Some("py")
            || !in_root
            || origin.components().any(|component| {
                matches!(
                    component.as_os_str().to_str(),
                    Some(".venv" | "venv" | ".tox" | ".nox" | "site-packages")
                )
            })
        {
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
        let plans = collect_plans(&source, current_python_version(py));
        let Some(plans) = plans else {
            self.original
                .bind(py)
                .call_method1("exec_module", (module,))?;
            return Ok(());
        };
        if plans.is_empty() {
            self.original
                .bind(py)
                .call_method1("exec_module", (module,))?;
            return Ok(());
        }
        let ast = py.import("ast")?;
        let dict = module.getattr("__dict__")?.cast_into::<PyDict>()?;
        compile_and_exec(py, &ast, &source, &self.filename, &dict, plans)
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

/// Installs the first-party assertion finder for a project root.
pub fn install(py: Python<'_>, root: &Utf8Path) -> PyResult<()> {
    let roots = if let Some(existing) = py
        .import("sys")?
        .getattr("meta_path")?
        .cast::<PyList>()?
        .iter()
        .find(|item| item.is_instance_of::<AssertionFinder>())
    {
        existing.cast::<AssertionFinder>()?.borrow().roots.clone()
    } else {
        let roots = Arc::new(Mutex::new(BTreeSet::new()));
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
        .lock()
        .map_err(|_| pyo3::exceptions::PyRuntimeError::new_err("assertion roots poisoned"))?
        .insert(
            std::fs::canonicalize(root.as_std_path())
                .unwrap_or_else(|_| root.as_std_path().to_path_buf()),
        );
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

/// Renders an assertion explanation from the failing frame.
///
/// The formatter is added after the loader path is proven against the focused
/// import, scope, and optimization regressions.
pub(crate) fn explain(_py: Python<'_>, _error: &PyErr) -> Option<String> {
    let py = _py;
    let mut traceback = _error.traceback(py)?.into_any();
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
    let filename = std::fs::canonicalize(&filename)
        .unwrap_or_else(|_| PathBuf::from(&filename))
        .to_string_lossy()
        .into_owned();
    let registry = registry().read().ok()?;
    let metadata = registry.get(&filename)?;
    let column = instruction_column(&frame, &traceback);
    let candidates = metadata
        .iter()
        .filter(|metadata| metadata.location.line == line)
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
