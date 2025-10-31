use std::collections::HashMap;

use pyo3::prelude::*;

type ArgNames = Vec<String>;
type ArgValues = Vec<Vec<Py<PyAny>>>;

/// Parse parametrize arguments from Python objects.
///
/// This helper function handles multiple input formats:
/// - `("arg1, arg2", [(1, 2), (3, 4)])` - single arg name with values (wrapped into Vec<Vec>)
/// - `("arg1", [3, 4])` - comma-separated arg names (re-extracted as Vec<Vec>)
/// - `(["arg1", "arg2"], [(1, 2), (3, 4)])` - direct arg names and nested values
pub(super) fn parse_parametrize_args(
    arg_names: &Bound<'_, PyAny>,
    arg_values: &Bound<'_, PyAny>,
) -> Result<(ArgNames, ArgValues), ()> {
    // Try extracting as (String, Vec<Py<PyAny>>)
    if let (Ok(name), Ok(values)) = (
        arg_names.extract::<String>(),
        arg_values.extract::<Vec<Py<PyAny>>>(),
    ) {
        // Check if the string contains comma-separated argument names
        if name.contains(',') {
            let names = name.split(',').map(|s| s.trim().to_string()).collect();
            let values = arg_values
                .extract::<Vec<Vec<Py<PyAny>>>>()
                .map_err(|_| ())?;
            Ok((names, values))
        } else {
            // Single argument name - wrap each value in a Vec
            let values = values.into_iter().map(|v| vec![v]).collect();
            Ok((vec![name], values))
        }
    } else if let (Ok(names), Ok(values)) = (
        arg_names.extract::<Vec<String>>(),
        arg_values.extract::<Vec<Vec<Py<PyAny>>>>(),
    ) {
        // Direct extraction of Vec<String> and Vec<Vec<Py<PyAny>>>
        Ok((names, values))
    } else {
        Err(())
    }
}

/// Represents different argument names and values that can be given to a test.
///
/// This is most useful to repeat a test multiple times with different arguments instead of duplicating the test.
#[derive(Debug, Clone)]
pub struct ParametrizeTag {
    /// The names of the arguments
    ///
    /// These are used as keyword argument names for the test function.
    arg_names: ArgNames,

    /// The values associated with each argument name.
    arg_values: ArgValues,
}

impl ParametrizeTag {
    pub(crate) const fn new(arg_names: ArgNames, arg_values: ArgValues) -> Self {
        Self {
            arg_names,
            arg_values,
        }
    }

    fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> Option<Self> {
        let args = py_mark.getattr("args").ok()?;
        // Extract first two elements from args tuple
        let arg_names = args.get_item(0).ok()?;
        let arg_values = args.get_item(1).ok()?;

        let (arg_names, arg_values) = parse_parametrize_args(&arg_names, &arg_values).ok()?;

        Some(Self {
            arg_names,
            arg_values,
        })
    }

    /// Returns each parameterize case.
    ///
    /// Each [`HashMap`] is used as keyword arguments for the test function.
    pub(crate) fn each_arg_value(&self) -> Vec<HashMap<String, Py<PyAny>>> {
        let total_combinations = self.arg_values.len();
        let mut param_args = Vec::with_capacity(total_combinations);

        for values in &self.arg_values {
            let mut current_parameratisation = HashMap::with_capacity(self.arg_names.len());
            for (arg_name, arg_value) in self.arg_names.iter().zip(values.iter()) {
                current_parameratisation.insert(arg_name.clone(), arg_value.clone());
            }
            param_args.push(current_parameratisation);
        }
        param_args
    }
}

impl TryFrom<&Bound<'_, PyAny>> for ParametrizeTag {
    type Error = ();

    fn try_from(py_mark: &Bound<'_, PyAny>) -> Result<Self, Self::Error> {
        Self::try_from_pytest_mark(py_mark).ok_or(())
    }
}
