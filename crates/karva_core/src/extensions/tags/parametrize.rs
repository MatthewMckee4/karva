use std::collections::HashMap;

use pyo3::{IntoPyObjectExt, prelude::*};

use crate::extensions::{functions::Param, tags::Tags};

/// A single parametrization of a function
#[derive(Debug, Clone)]
pub struct Parametrization {
    /// The values of the arguments
    ///
    /// These are used as values for the test function.
    pub(crate) values: Vec<Py<PyAny>>,

    /// Tags associated with this parametrization
    pub(crate) tags: Tags,
}

impl Parametrization {
    pub(crate) const fn tags(&self) -> &Tags {
        &self.tags
    }
}

impl From<PyRef<'_, Param>> for Parametrization {
    fn from(param: PyRef<'_, Param>) -> Self {
        Self {
            values: param.values().clone(),
            tags: param.tags().clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ParametrizationArgs {
    pub(crate) values: HashMap<String, Py<PyAny>>,

    pub(crate) tags: Tags,
}

impl ParametrizationArgs {
    pub(crate) const fn values(&self) -> &HashMap<String, Py<PyAny>> {
        &self.values
    }

    pub(crate) fn extend(&mut self, other: Self) {
        self.values.extend(other.values);
        self.tags.extend(&other.tags);
    }
}

/// Parse parametrize arguments from Python objects.
///
/// This helper function handles multiple input formats:
/// - `("arg1, arg2", [(1, 2), (3, 4)])` - single arg name with values (wrapped into Vec<Vec>)
/// - `("arg1", [3, 4])` - comma-separated arg names (re-extracted as Vec<Vec>)
/// - `(["arg1", "arg2"], [(1, 2), (3, 4)])` - direct arg names and nested values
/// - `(["arg1", "arg2"], [pytest.param(1, 2), pytest.param(3, 4)])` - direct arg names and single values
/// - `(["arg1"], [pytest.param(1), pytest.param(3)])` - direct arg names and single values
pub(super) fn parse_parametrize_args(
    arg_names: &Bound<'_, PyAny>,
    arg_values: &Bound<'_, PyAny>,
) -> Option<(Vec<String>, Vec<Parametrization>)> {
    let py = arg_values.py();

    // Try extracting as (String, Vec<Py<PyAny>>)
    if let (Ok(name), Ok(values)) = (
        arg_names.extract::<String>(),
        arg_values.extract::<Vec<Py<PyAny>>>(),
    ) {
        // Check if the string contains comma-separated argument names
        if name.contains(',') {
            let names: Vec<String> = name.split(',').map(|s| s.trim().to_string()).collect();
            let parametrizations = arg_values
                .extract::<Vec<Py<PyAny>>>()
                .ok()?
                .into_iter()
                .map(|param| handle_custom_parametrize_param(py, param, true))
                .collect();

            Some((names, parametrizations))
        } else {
            // Single argument name - wrap each value in a Vec
            let parametrizations = values
                .into_iter()
                .map(|param| handle_custom_parametrize_param(py, param, false))
                .collect();

            Some((vec![name], parametrizations))
        }
    } else if let (Ok(names), Ok(values)) = (
        arg_names.extract::<Vec<String>>(),
        arg_values.extract::<Vec<Py<PyAny>>>(),
    ) {
        let parametrizations = values
            .into_iter()
            .map(|param| handle_custom_parametrize_param(py, param, true))
            .collect();
        // Direct extraction of Vec<String> and Vec<Vec<Py<PyAny>>>
        Some((names, parametrizations))
    } else {
        None
    }
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

impl ParametrizeTag {
    pub(crate) const fn new(names: Vec<String>, parametrizations: Vec<Parametrization>) -> Self {
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
                .map(|Param { values, tags }| Parametrization { values, tags })
                .collect(),
        )
    }

    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let args = py_mark.getattr("args").ok()?;
        // Extract first two elements from args tuple
        let arg_names = args.get_item(0).ok()?;
        let arg_values = args.get_item(1).ok()?;

        let (arg_names, parametrizations) = parse_parametrize_args(&arg_names, &arg_values)?;

        Some(Self::new(arg_names, parametrizations))
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
                current_parameratisation.insert(arg_name.clone(), arg_value.clone());
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
    let default_parametrization = || Parametrization {
        values: vec![param.clone()],
        tags: Tags::default(),
    };

    if let Ok(param) = param.cast_bound::<Param>(py) {
        let param = param.borrow();
        return Parametrization::from(param);
    }

    let Ok(bound_param) = param.clone().into_bound_py_any(py) else {
        return default_parametrization();
    };

    let a_type = bound_param.get_type();

    let Ok(type_name) = a_type.name() else {
        return default_parametrization();
    };

    let Some(type_name_str) = type_name.to_str().ok() else {
        return default_parametrization();
    };

    if type_name_str.contains("ParameterSet") {
        // Handle pytest.param - extract the values attribute
        let Ok(values_attr) = bound_param.getattr("values") else {
            return default_parametrization();
        };

        // The values attribute is a tuple - extract it as a list
        let values = values_attr
            .extract::<Vec<Py<PyAny>>>()
            .unwrap_or_else(|_| vec![param]);

        let Ok(marks) = bound_param.getattr("marks") else {
            return Parametrization {
                values,
                tags: Tags::default(),
            };
        };

        let Ok(marks) = marks.into_py_any(py) else {
            return Parametrization {
                values,
                tags: Tags::default(),
            };
        };

        let tags = Tags::from_pytest_marks(py, &marks).unwrap_or_default();

        Parametrization { values, tags }
    } else if expect_multiple && let Ok(params) = bound_param.extract::<Vec<Py<PyAny>>>() {
        Parametrization {
            values: params,
            tags: Tags::default(),
        }
    } else {
        default_parametrization()
    }
}
