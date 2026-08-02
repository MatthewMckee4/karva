use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use pyo3::IntoPyObjectExt;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use ruff_python_ast::{Expr, StmtFunctionDef};
use ruff_python_stdlib::identifiers::is_identifier;
use ruff_text_size::{Ranged, TextRange};
use thiserror::Error;

use crate::extensions::functions::Param;
use crate::extensions::tags::Tags;
use crate::utils::display_value;

/// Source annotation attached to an invalid `parametrize` diagnostic.
#[derive(Debug)]
pub struct ParametrizeAnnotation {
    /// Exact source range to underline.
    pub(crate) range: TextRange,

    /// Label rendered beside the underline.
    pub(crate) message: String,
}

impl ParametrizeAnnotation {
    fn new(range: TextRange, message: impl Into<String>) -> Self {
        Self {
            range,
            message: message.into(),
        }
    }
}

/// Primary and supporting source locations for a parametrization error.
#[derive(Debug)]
pub struct ParametrizeDiagnosticLocation {
    /// Location that receives the main diagnostic label.
    pub(crate) primary: ParametrizeAnnotation,

    /// Related locations explaining conflicts or expected structure.
    pub(crate) secondary: Vec<ParametrizeAnnotation>,
}

const fn value_noun(count: usize) -> &'static str {
    if count == 1 { "value" } else { "values" }
}

/// Validation failures shared by native and pytest-compatible parametrization.
#[derive(Debug, Error)]
pub enum InvalidParametrizeError {
    #[error("Parameter name cannot be empty")]
    EmptyName,
    #[error("`{name}` is not a valid Python identifier")]
    InvalidName { name: String },
    #[error("Parameter `{name}` is parametrized more than once")]
    DuplicateName { name: String },
    #[error("Parametrization has no cases")]
    EmptyCases { names: String },
    #[error(
        "Expected {expected} {} for `{names}`, but case {case} contains {actual} {}",
        value_noun(*expected),
        value_noun(*actual)
    )]
    WrongArity {
        names: String,
        case: usize,
        expected: usize,
        actual: usize,
    },
    #[error("Parameter `{name}` does not exist in the test function signature")]
    UnknownName { name: String },
    #[error("Parameter ID cannot be empty")]
    EmptyId,
}

struct ParametrizeSyntax<'a> {
    names: &'a Expr,
    values: &'a Expr,
    has_known_callee: bool,
}

impl InvalidParametrizeError {
    /// Maps semantic failure data back onto decorator expressions when unambiguous.
    pub fn diagnostic_location(
        &self,
        function: &StmtFunctionDef,
    ) -> Option<ParametrizeDiagnosticLocation> {
        let syntaxes = parametrize_syntaxes(function);

        match self {
            Self::EmptyName => {
                name_diagnostic_location(&syntaxes, "", "empty parameter name", function)
            }
            Self::InvalidName { name } => {
                name_diagnostic_location(&syntaxes, name, "invalid parameter name", function)
            }
            Self::DuplicateName { name } => {
                let matching = syntaxes
                    .iter()
                    .map(|syntax| (syntax, name_ranges(syntax.names, name)))
                    .filter(|(_, ranges)| !ranges.is_empty())
                    .collect::<Vec<_>>();
                let known_range_count = matching
                    .iter()
                    .filter(|(syntax, _)| syntax.has_known_callee)
                    .map(|(_, ranges)| ranges.len())
                    .sum::<usize>();

                let mut ranges: Vec<TextRange> = Vec::new();
                for (_, syntax_ranges) in matching
                    .iter()
                    .filter(|(syntax, _)| known_range_count < 2 || syntax.has_known_callee)
                {
                    for range in syntax_ranges {
                        if !ranges.contains(range) {
                            ranges.push(*range);
                        }
                    }
                }

                let primary = ranges.pop()?;
                Some(ParametrizeDiagnosticLocation {
                    primary: ParametrizeAnnotation::new(primary, "duplicate parameter"),
                    secondary: ranges
                        .into_iter()
                        .map(|range| ParametrizeAnnotation::new(range, "first parametrized here"))
                        .collect(),
                })
            }
            Self::EmptyCases { names } => {
                let syntax = select_matching_syntax(&syntaxes, names, |syntax| {
                    sequence_elements(syntax.values).is_some_and(<[Expr]>::is_empty)
                })?;
                Some(ParametrizeDiagnosticLocation {
                    primary: ParametrizeAnnotation::new(syntax.values.range(), "no cases provided"),
                    secondary: vec![ParametrizeAnnotation::new(
                        syntax.names.range(),
                        "parameters declared here",
                    )],
                })
            }
            Self::WrongArity {
                names,
                case,
                expected,
                actual,
            } => {
                let syntax = select_matching_syntax(&syntaxes, names, |syntax| {
                    case_expression(syntax, *case)
                        .and_then(expression_arity)
                        .is_some_and(|arity| arity == *actual)
                })?;
                let (primary, primary_message) =
                    if let Some(expression) = case_expression(syntax, *case) {
                        (
                            expression.range(),
                            format!("contains {actual} {}", value_noun(*actual)),
                        )
                    } else {
                        (
                            syntax.values.range(),
                            format!("case {case} contains {actual} {}", value_noun(*actual)),
                        )
                    };
                Some(ParametrizeDiagnosticLocation {
                    primary: ParametrizeAnnotation::new(primary, primary_message),
                    secondary: vec![ParametrizeAnnotation::new(
                        syntax.names.range(),
                        format!("expects {expected} {}", value_noun(*expected)),
                    )],
                })
            }
            Self::UnknownName { name } => {
                name_diagnostic_location(&syntaxes, name, "not accepted by test", function)
            }
            Self::EmptyId => None,
        }
    }
}

fn name_diagnostic_location(
    syntaxes: &[ParametrizeSyntax<'_>],
    name: &str,
    primary_message: &str,
    function: &StmtFunctionDef,
) -> Option<ParametrizeDiagnosticLocation> {
    let matching = syntaxes
        .iter()
        .filter(|syntax| !name_ranges(syntax.names, name).is_empty())
        .collect::<Vec<_>>();
    let syntax = select_unambiguous_syntax(matching)?;
    let primary = name_ranges(syntax.names, name).into_iter().next()?;
    let parameter_count = function.parameters.iter_non_variadic_params().count();
    let parameters_message = match parameter_count {
        0 => "test accepts no parameters",
        1 => "available parameter",
        _ => "available parameters",
    };
    Some(ParametrizeDiagnosticLocation {
        primary: ParametrizeAnnotation::new(primary, primary_message),
        secondary: vec![ParametrizeAnnotation::new(
            function.parameters.range,
            parameters_message,
        )],
    })
}

fn parametrize_syntaxes(function: &StmtFunctionDef) -> Vec<ParametrizeSyntax<'_>> {
    function
        .decorator_list
        .iter()
        .filter_map(|decorator| {
            let Expr::Call(call) = &decorator.expression else {
                return None;
            };
            let names = call
                .arguments
                .find_argument_value("argnames", 0)
                .or_else(|| call.arguments.find_argument_value("arg_names", 0))?;
            let values = call
                .arguments
                .find_argument_value("argvalues", 1)
                .or_else(|| call.arguments.find_argument_value("arg_values", 1))?;
            Some(ParametrizeSyntax {
                names,
                values,
                has_known_callee: is_parametrize_function(&call.func),
            })
        })
        .collect()
}

fn is_parametrize_function(expression: &Expr) -> bool {
    match expression {
        Expr::Name(name) => name.id == "parametrize",
        Expr::Attribute(attribute) => attribute.attr.id == "parametrize",
        _ => false,
    }
}

/// Selects the decorator whose literal names and error-specific shape match.
///
/// Dynamic syntax cannot always be resolved from the AST. In that case, a
/// location is returned only when the decorator call is otherwise unambiguous.
fn select_matching_syntax<'a, 'ast>(
    syntaxes: &'a [ParametrizeSyntax<'ast>],
    names: &str,
    predicate: impl Fn(&ParametrizeSyntax<'_>) -> bool,
) -> Option<&'a ParametrizeSyntax<'ast>> {
    let matching_names = syntaxes
        .iter()
        .filter(|syntax| {
            normalized_names(syntax.names)
                .is_some_and(|syntax_names| syntax_names.join(",") == names)
        })
        .collect::<Vec<_>>();

    let matching_error = matching_names
        .iter()
        .copied()
        .filter(|syntax| predicate(syntax))
        .collect::<Vec<_>>();
    select_unambiguous_syntax(matching_error)
        .or_else(|| select_unambiguous_syntax(matching_names))
        .or_else(|| {
            select_unambiguous_syntax(syntaxes.iter().filter(|syntax| predicate(syntax)).collect())
        })
        .or_else(|| select_unambiguous_syntax(syntaxes.iter().collect()))
}

fn select_unambiguous_syntax<'a, 'ast>(
    syntaxes: Vec<&'a ParametrizeSyntax<'ast>>,
) -> Option<&'a ParametrizeSyntax<'ast>> {
    if syntaxes.len() == 1 {
        return syntaxes.first().copied();
    }

    let mut recognized = syntaxes
        .into_iter()
        .filter(|syntax| syntax.has_known_callee);
    let syntax = recognized.next()?;
    recognized.next().is_none().then_some(syntax)
}

fn normalized_names(expression: &Expr) -> Option<Vec<&str>> {
    match expression {
        Expr::StringLiteral(string) => {
            Some(string.value.to_str().split(',').map(str::trim).collect())
        }
        Expr::List(list) => list.elts.iter().map(string_value).collect(),
        Expr::Tuple(tuple) => tuple.elts.iter().map(string_value).collect(),
        _ => None,
    }
}

fn string_value(expression: &Expr) -> Option<&str> {
    let Expr::StringLiteral(string) = expression else {
        return None;
    };
    Some(string.value.to_str())
}

fn name_ranges(expression: &Expr, target: &str) -> Vec<TextRange> {
    match expression {
        Expr::StringLiteral(string) => string
            .value
            .to_str()
            .split(',')
            .map(str::trim)
            .filter(|name| *name == target)
            .map(|_| expression.range())
            .collect(),
        Expr::List(list) => list
            .elts
            .iter()
            .filter(|element| string_value(element) == Some(target))
            .map(Ranged::range)
            .collect(),
        Expr::Tuple(tuple) => tuple
            .elts
            .iter()
            .filter(|element| string_value(element) == Some(target))
            .map(Ranged::range)
            .collect(),
        _ => Vec::new(),
    }
}

fn sequence_elements(expression: &Expr) -> Option<&[Expr]> {
    match expression {
        Expr::List(list) => Some(&list.elts),
        Expr::Tuple(tuple) => Some(&tuple.elts),
        _ => None,
    }
}

fn case_expression<'a>(syntax: &'a ParametrizeSyntax<'_>, case: usize) -> Option<&'a Expr> {
    sequence_elements(syntax.values)?.get(case - 1)
}

fn expression_arity(expression: &Expr) -> Option<usize> {
    match expression {
        Expr::List(list) => Some(list.elts.len()),
        Expr::Tuple(tuple) => Some(tuple.elts.len()),
        Expr::Call(call) if is_param_function(&call.func) => Some(call.arguments.args.len()),
        Expr::Name(_) | Expr::Attribute(_) | Expr::Subscript(_) | Expr::Call(_) => None,
        _ => Some(1),
    }
}

fn is_param_function(expression: &Expr) -> bool {
    match expression {
        Expr::Name(name) => name.id == "param",
        Expr::Attribute(attribute) => attribute.attr.id == "param",
        _ => false,
    }
}

/// A single set of parameter values for a parametrized test.
///
/// Represents one "row" of test data that will be used to run the test
/// function once, along with any tags specific to this parameter set.
#[derive(Debug, Clone)]
pub struct Parametrization {
    /// The argument values for this test variant.
    pub(crate) values: Vec<Arc<Py<PyAny>>>,

    /// Tags specific to this parameter set (e.g., marks on pytest.param).
    pub(crate) tags: Tags,

    /// Explicit display ID.
    pub(crate) id: Option<String>,

    /// Value-derived display ID for partially explicit stacked parametrization.
    pub(crate) default_id: String,
}

impl Parametrization {
    pub(crate) fn tags(&self) -> &Tags {
        &self.tags
    }
}

impl From<PyRef<'_, Param>> for Parametrization {
    fn from(param: PyRef<'_, Param>) -> Self {
        Self {
            values: param.values.clone(),
            tags: param.tags.clone(),
            id: param.id.clone(),
            default_id: param.default_id.clone(),
        }
    }
}

/// Named parameter values for a single test invocation.
///
/// Maps parameter names to their values, combining multiple
/// `@parametrize` decorators into a single set of arguments.
#[derive(Debug, Clone, Default)]
pub struct ParametrizationArgs {
    /// Mapping of parameter name to its value.
    pub(crate) values: HashMap<String, Arc<Py<PyAny>>>,

    /// Combined tags from all parameter sets.
    pub(crate) tags: Tags,

    id: String,
    has_explicit_id: bool,
}

impl ParametrizationArgs {
    pub(crate) fn extend(&mut self, other: Self) {
        self.values.extend(other.values);
        self.tags.extend(&other.tags);
        if !self.id.is_empty() && !other.id.is_empty() {
            self.id.push('-');
        }
        self.id.push_str(&other.id);
        self.has_explicit_id |= other.has_explicit_id;
    }

    pub(crate) fn id(&self) -> Option<&str> {
        self.has_explicit_id.then_some(self.id.as_str())
    }
}

/// Lazily expanded Cartesian product of stacked parametrization decorators.
#[derive(Debug)]
pub struct ParameterPlan {
    dimensions: Vec<Vec<ParametrizationArgs>>,
}

impl ParameterPlan {
    pub(crate) fn new(dimensions: Vec<Vec<ParametrizationArgs>>) -> Self {
        Self { dimensions }
    }
}

impl IntoIterator for ParameterPlan {
    type Item = ParametrizationArgs;
    type IntoIter = ParameterPlanIterator;

    fn into_iter(self) -> Self::IntoIter {
        let complete = self.dimensions.iter().any(Vec::is_empty);
        let indices = vec![0; self.dimensions.len()];
        ParameterPlanIterator {
            dimensions: self.dimensions,
            indices,
            complete,
        }
    }
}

/// Mixed-radix iterator that materializes only the next parameter combination.
pub struct ParameterPlanIterator {
    dimensions: Vec<Vec<ParametrizationArgs>>,
    indices: Vec<usize>,
    complete: bool,
}

impl Iterator for ParameterPlanIterator {
    type Item = ParametrizationArgs;

    fn next(&mut self) -> Option<Self::Item> {
        if self.complete {
            return None;
        }

        if self.dimensions.is_empty() {
            self.complete = true;
            return Some(ParametrizationArgs::default());
        }

        let mut combination = ParametrizationArgs::default();
        for (dimension, index) in self.dimensions.iter().zip(&self.indices) {
            combination.extend(dimension[*index].clone());
        }

        self.advance();
        Some(combination)
    }
}

impl ParameterPlanIterator {
    fn advance(&mut self) {
        for dimension_index in (0..self.indices.len()).rev() {
            self.indices[dimension_index] += 1;
            if self.indices[dimension_index] < self.dimensions[dimension_index].len() {
                return;
            }
            self.indices[dimension_index] = 0;
        }
        self.complete = true;
    }
}

fn make_unique_parametrize_ids(parametrizations: &mut [ParametrizationArgs]) {
    let mut id_counts = HashMap::new();
    for parametrization in &*parametrizations {
        if !parametrization.id.is_empty() {
            *id_counts.entry(parametrization.id.clone()).or_insert(0) += 1;
        }
    }

    let mut used_ids = parametrizations
        .iter()
        .map(|parametrization| parametrization.id.clone())
        .collect::<HashSet<_>>();
    let mut id_suffixes = HashMap::new();
    for parametrization in parametrizations {
        if id_counts.get(&parametrization.id).copied().unwrap_or(0) < 2 {
            continue;
        }

        let id = &parametrization.id;
        let separator = if id.ends_with(|character: char| character.is_ascii_digit()) {
            "_"
        } else {
            ""
        };
        let counter = id_suffixes.entry(id.clone()).or_insert(0);
        loop {
            let candidate = format!("{id}{separator}{counter}");
            *counter += 1;
            if used_ids.insert(candidate.clone()) {
                parametrization.id = candidate;
                break;
            }
        }
    }
}

/// Builds pytest-compatible display identity from Python values.
pub fn default_param_id(py: Python<'_>, values: &[Arc<Py<PyAny>>]) -> String {
    values
        .iter()
        .filter_map(|value| value.cast_bound::<PyAny>(py).ok())
        .map(display_value)
        .collect::<Vec<_>>()
        .join("-")
}

/// Normalize argument names from Python into a `Vec<String>`.
///
/// Handles both input formats for parameter names:
/// - A list of strings: `["arg1", "arg2"]`
/// - A single comma-separated string: `"arg1, arg2"` or just `"arg1"`
fn normalize_arg_names(arg_names: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    if let Ok(names) = arg_names.extract::<Vec<String>>() {
        return Ok(names);
    }
    if let Ok(name) = arg_names.extract::<String>() {
        return Ok(name.split(',').map(|s| s.trim().to_string()).collect());
    }
    Err(PyValueError::new_err(
        "pytest parametrize mark argnames must be a string or list of strings",
    ))
}

/// Parse parametrize arguments from Python objects.
///
/// This helper function handles multiple input formats:
/// - `("arg1, arg2", [(1, 2), (3, 4)])` - comma-separated arg names with tuple values
/// - `("arg1", [3, 4])` - single arg name with scalar values
/// - `(["arg1", "arg2"], [(1, 2), (3, 4)])` - list of arg names with tuple values
/// - `(["arg1", "arg2"], [pytest.param(1, 2), ...])` - list of arg names with param values
/// - `(["arg1"], [pytest.param(1), ...])` - single-element list with param values
pub(super) fn parse_parametrize_args(
    arg_names: &Bound<'_, PyAny>,
    arg_values: &Bound<'_, PyAny>,
    globals: Option<&Bound<'_, PyDict>>,
) -> PyResult<(Vec<String>, Vec<Parametrization>)> {
    let py = arg_values.py();
    let names = normalize_arg_names(arg_names)?;
    let values = arg_values
        .try_iter()
        .map_err(|_| {
            PyValueError::new_err("pytest parametrize mark argvalues must be an iterable")
        })?
        .map(|value| value.map(Bound::unbind))
        .collect::<PyResult<Vec<_>>>()?;
    let expect_multiple = names.len() > 1;
    let parametrizations = values
        .into_iter()
        .map(|param| handle_custom_parametrize_param(py, param, expect_multiple, globals))
        .collect::<PyResult<Vec<_>>>()?;
    Ok((names, parametrizations))
}

/// Applies an ID sequence or callable without replacing IDs embedded in `param(...)` values.
pub(super) fn apply_parametrize_ids(
    ids: Option<&Bound<'_, PyAny>>,
    parametrizations: &mut [Parametrization],
) -> PyResult<()> {
    let Some(ids) = ids else {
        return Ok(());
    };
    if ids.is_none() {
        return Ok(());
    }
    if ids.is_callable() {
        let py = ids.py();
        for parametrization in parametrizations {
            if parametrization.id.is_some() {
                continue;
            }
            let mut parts = Vec::with_capacity(parametrization.values.len());
            for value in &parametrization.values {
                let generated = ids.call1((value.bind(py),))?;
                if generated.is_none() {
                    parts.push(display_value(value.bind(py)));
                } else {
                    parts.push(generated.to_string());
                }
            }
            parametrization.id = Some(parts.join("-"));
        }
        return Ok(());
    }
    let ids = ids.extract::<Vec<Option<String>>>().map_err(|_| {
        PyValueError::new_err("parametrize ids must be a sequence of strings or None")
    })?;
    if ids.is_empty() {
        return Ok(());
    }
    if ids.len() != parametrizations.len() {
        return Err(PyValueError::new_err(format!(
            "{} parameter sets specified, with different number of ids: {}",
            parametrizations.len(),
            ids.len()
        )));
    }
    for (parametrization, id) in parametrizations.iter_mut().zip(ids) {
        if parametrization.id.is_none() {
            parametrization.id = id;
        }
    }
    Ok(())
}

/// Represents different argument names and values that can be given to a test.
///
/// This is most useful to repeat a test multiple times with different arguments instead of duplicating the test.
#[derive(Debug, Clone)]
pub struct ParametrizeTag {
    /// The names and values of the arguments
    ///
    /// These are used as keyword argument names for the test function.
    names: Vec<String>,
    parametrizations: Vec<Parametrization>,
}

/// Extract argnames and argvalues from a pytest parametrize mark.
///
/// Handles both positional args and keyword arguments in any combination:
/// - `@pytest.mark.parametrize("x", [1, 2])` - both positional
/// - `@pytest.mark.parametrize(argnames="x", argvalues=[1, 2])` - both kwargs
/// - `@pytest.mark.parametrize("x", argvalues=[1, 2])` - mixed
/// - `@pytest.mark.parametrize(argnames="x", [1, 2])` - mixed
struct ExtractedParametrizeArgs<'py> {
    names: Bound<'py, PyAny>,
    values: Bound<'py, PyAny>,
    ids: Option<Bound<'py, PyAny>>,
}

fn extract_parametrize_args<'py>(
    py_mark: &Bound<'py, PyAny>,
) -> PyResult<ExtractedParametrizeArgs<'py>> {
    // Try to get argnames from positional args first, then kwargs
    let arg_names = py_mark
        .getattr("args")
        .and_then(|args| args.get_item(0))
        .or_else(|_| {
            py_mark
                .getattr("kwargs")
                .and_then(|kwargs| kwargs.get_item("argnames"))
        })
        .map_err(|_| PyValueError::new_err("pytest parametrize mark requires argnames"))?;

    // Try to get argvalues from positional args second position, then kwargs
    let arg_values = py_mark
        .getattr("args")
        .and_then(|args| args.get_item(1))
        .or_else(|_| {
            py_mark
                .getattr("kwargs")
                .and_then(|kwargs| kwargs.get_item("argvalues"))
        })
        .map_err(|_| PyValueError::new_err("pytest parametrize mark requires argvalues"))?;

    let ids = py_mark
        .getattr("args")
        .and_then(|args| args.get_item(3))
        .or_else(|_| {
            py_mark
                .getattr("kwargs")
                .and_then(|kwargs| kwargs.get_item("ids"))
        })
        .ok();

    Ok(ExtractedParametrizeArgs {
        names: arg_names,
        values: arg_values,
        ids,
    })
}

impl ParametrizeTag {
    pub(crate) fn names(&self) -> &[String] {
        &self.names
    }

    pub(crate) fn new(names: Vec<String>, parametrizations: Vec<Parametrization>) -> Self {
        Self {
            names,
            parametrizations,
        }
    }

    pub(crate) fn from_karva(arg_names: Vec<String>, arg_values: Vec<Param>) -> Self {
        Self::new(
            arg_names,
            arg_values
                .into_iter()
                .map(
                    |Param {
                         values: param_values,
                         tags,
                         id,
                         default_id,
                     }| Parametrization {
                        values: param_values,
                        tags,
                        id,
                        default_id,
                    },
                )
                .collect(),
        )
    }

    pub(crate) fn try_from_pytest_mark(
        py_mark: &Bound<'_, PyAny>,
        globals: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<Self>> {
        let ExtractedParametrizeArgs { names, values, ids } = extract_parametrize_args(py_mark)?;

        let (arg_names, mut parametrizations) = parse_parametrize_args(&names, &values, globals)?;
        apply_parametrize_ids(ids.as_ref(), &mut parametrizations)?;

        Ok(Some(Self::new(arg_names, parametrizations)))
    }

    pub(crate) fn validate<'a>(
        &'a self,
        function_parameter_names: &HashSet<&str>,
        seen_names: &mut HashSet<&'a str>,
    ) -> Result<(), InvalidParametrizeError> {
        for name in &self.names {
            if name.is_empty() {
                return Err(InvalidParametrizeError::EmptyName);
            }
            if !is_identifier(name) {
                return Err(InvalidParametrizeError::InvalidName { name: name.clone() });
            }
            if !seen_names.insert(name) {
                return Err(InvalidParametrizeError::DuplicateName { name: name.clone() });
            }
        }

        if self.parametrizations.is_empty() {
            return Err(InvalidParametrizeError::EmptyCases {
                names: self.names.join(","),
            });
        }

        let expected = self.names.len();
        let names = self.names.join(",");
        for (index, parametrization) in self.parametrizations.iter().enumerate() {
            let actual = parametrization.values.len();
            if actual != expected {
                return Err(InvalidParametrizeError::WrongArity {
                    names,
                    case: index + 1,
                    expected,
                    actual,
                });
            }
        }

        for name in &self.names {
            if !function_parameter_names.contains(name.as_str()) {
                return Err(InvalidParametrizeError::UnknownName { name: name.clone() });
            }
        }

        Ok(())
    }

    /// Returns each parameterize case.
    ///
    /// Each [`HashMap`] is used as keyword arguments for the test function.
    pub(crate) fn each_arg_value(&self) -> Vec<ParametrizationArgs> {
        let total_combinations = self.parametrizations.len();
        let mut param_args = Vec::with_capacity(total_combinations);

        for parametrization in &self.parametrizations {
            let mut current_parameratisation = HashMap::with_capacity(self.names.len());
            for (arg_name, arg_value) in self.names.iter().zip(parametrization.values.iter()) {
                current_parameratisation.insert(arg_name.clone(), Arc::clone(arg_value));
            }
            let current_param_args = ParametrizationArgs {
                values: current_parameratisation,
                tags: parametrization.tags().clone(),
                id: parametrization
                    .id
                    .clone()
                    .unwrap_or_else(|| parametrization.default_id.clone()),
                has_explicit_id: parametrization.id.is_some(),
            };
            param_args.push(current_param_args);
        }
        make_unique_parametrize_ids(&mut param_args);
        param_args
    }
}

/// Check for instances of `pytest.ParameterSet` and extract the parameters
/// from it. Also handles regular tuples by extracting their values.
pub(super) fn handle_custom_parametrize_param(
    py: Python,
    param: Py<PyAny>,
    expect_multiple: bool,
    globals: Option<&Bound<'_, PyDict>>,
) -> PyResult<Parametrization> {
    let param_arc = Arc::new(param);
    let default_parametrization = || Parametrization {
        values: vec![Arc::clone(&param_arc)],
        tags: Tags::default(),
        id: None,
        default_id: default_param_id(py, &[Arc::clone(&param_arc)]),
    };

    if let Ok(param_bound) = param_arc.cast_bound::<Param>(py) {
        let param_ref = param_bound.borrow();
        return Ok(Parametrization::from(param_ref));
    }

    let Ok(bound_param) = param_arc.clone_ref(py).into_bound_py_any(py) else {
        return Ok(default_parametrization());
    };

    let is_parameter_set = match bound_param.get_type().name() {
        Ok(type_name) => match type_name.to_str() {
            Ok(type_name) => type_name.contains("ParameterSet"),
            Err(err) => {
                tracing::warn!("Failed to inspect parametrized value type name: {err}");
                false
            }
        },
        Err(err) => {
            tracing::warn!("Failed to inspect parametrized value type: {err}");
            false
        }
    };

    if is_parameter_set {
        let values: Vec<Arc<Py<PyAny>>> = bound_param
            .getattr("values")
            .and_then(|v| v.extract::<Vec<Py<PyAny>>>())
            .map(|v| v.into_iter().map(Arc::new).collect())
            .unwrap_or_else(|_| vec![Arc::clone(&param_arc)]);

        let tags = match bound_param.getattr("marks") {
            Ok(m) => {
                let marks = m.into_py_any(py)?;
                Tags::from_pytest_marks(py, &marks, globals)?.unwrap_or_default()
            }
            Err(err) => {
                tracing::warn!("Failed to inspect pytest.param marks: {err}");
                Tags::default()
            }
        };

        let parameter_id = bound_param.getattr("id")?;
        let id = if parameter_id.is_none() {
            None
        } else {
            Some(parameter_id.extract::<String>()?)
        };
        let default_id = default_param_id(py, &values);

        Ok(Parametrization {
            values,
            tags,
            id,
            default_id,
        })
    } else if expect_multiple && let Ok(params) = bound_param.extract::<Vec<Py<PyAny>>>() {
        let values = params.into_iter().map(Arc::new).collect::<Vec<_>>();
        Ok(Parametrization {
            default_id: default_param_id(py, &values),
            values,
            tags: Tags::default(),
            id: None,
        })
    } else {
        Ok(default_parametrization())
    }
}

#[cfg(test)]
mod tests {
    use super::{ParameterPlan, ParametrizationArgs};

    fn args(id: &str) -> ParametrizationArgs {
        ParametrizationArgs {
            id: id.to_string(),
            has_explicit_id: true,
            ..ParametrizationArgs::default()
        }
    }

    #[test]
    fn parameter_plan_preserves_stacked_decorator_order() {
        let combinations = ParameterPlan::new(vec![
            vec![args("a0"), args("a1")],
            vec![args("b0"), args("b1"), args("b2")],
        ])
        .into_iter()
        .filter_map(|arguments| arguments.id().map(str::to_string))
        .collect::<Vec<_>>();

        assert_eq!(
            combinations,
            ["a0-b0", "a0-b1", "a0-b2", "a1-b0", "a1-b1", "a1-b2"]
        );
    }

    #[test]
    fn parameter_plan_emits_one_case_without_parametrization() {
        let combinations = ParameterPlan::new(Vec::new())
            .into_iter()
            .collect::<Vec<_>>();

        assert_eq!(combinations.len(), 1);
        assert_eq!(combinations[0].id(), None);
    }
}
