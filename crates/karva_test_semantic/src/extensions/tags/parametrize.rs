use std::collections::HashMap;
use std::sync::Arc;

use pyo3::IntoPyObjectExt;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::extensions::functions::Param;
use crate::extensions::tags::Tags;

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
}

impl ParametrizationArgs {
    pub(crate) fn values(&self) -> &HashMap<String, Arc<Py<PyAny>>> {
        &self.values
    }

    pub(crate) fn extend(&mut self, other: Self) {
        self.values.extend(other.values);
        self.tags.extend(&other.tags);
    }
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
) -> PyResult<(Vec<String>, Vec<Parametrization>)> {
    let py = arg_values.py();
    let names = normalize_arg_names(arg_names)?;
    let values = arg_values.extract::<Vec<Py<PyAny>>>().map_err(|_| {
        PyValueError::new_err("pytest parametrize mark argvalues must be an iterable")
    })?;
    let expect_multiple = names.len() > 1;
    let parametrizations = values
        .into_iter()
        .map(|param| handle_custom_parametrize_param(py, param, expect_multiple))
        .collect();
    Ok((names, parametrizations))
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
fn extract_parametrize_args<'py>(
    py_mark: &Bound<'py, PyAny>,
) -> PyResult<(Bound<'py, PyAny>, Bound<'py, PyAny>)> {
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

    Ok((arg_names, arg_values))
}

impl ParametrizeTag {
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
                     }| Parametrization {
                        values: param_values,
                        tags,
                    },
                )
                .collect(),
        )
    }

    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> PyResult<Option<Self>> {
        let (arg_names, arg_values) = extract_parametrize_args(py_mark)?;

        let (arg_names, parametrizations) = parse_parametrize_args(&arg_names, &arg_values)?;

        Ok(Some(Self::new(arg_names, parametrizations)))
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
            };
            param_args.push(current_param_args);
        }
        param_args
    }
}

/// Check for instances of `pytest.ParameterSet` and extract the parameters
/// from it. Also handles regular tuples by extracting their values.
pub(super) fn handle_custom_parametrize_param(
    py: Python,
    param: Py<PyAny>,
    expect_multiple: bool,
) -> Parametrization {
    let param_arc = Arc::new(param);
    let default_parametrization = || Parametrization {
        values: vec![Arc::clone(&param_arc)],
        tags: Tags::default(),
    };

    if let Ok(param_bound) = param_arc.cast_bound::<Param>(py) {
        let param_ref = param_bound.borrow();
        return Parametrization::from(param_ref);
    }

    let Ok(bound_param) = param_arc.clone_ref(py).into_bound_py_any(py) else {
        return default_parametrization();
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

        let tags = bound_param
            .getattr("marks")
            .ok()
            .and_then(|m| m.into_py_any(py).ok())
            .and_then(|m| Tags::from_pytest_marks(py, &m).ok().flatten())
            .unwrap_or_default();

        Parametrization { values, tags }
    } else if expect_multiple && let Ok(params) = bound_param.extract::<Vec<Py<PyAny>>>() {
        Parametrization {
            values: params.into_iter().map(Arc::new).collect(),
            tags: Tags::default(),
        }
    } else {
        default_parametrization()
    }
}
