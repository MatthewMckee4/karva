use std::{collections::HashSet, sync::LazyLock};

use pyo3::{
    IntoPyObjectExt,
    prelude::*,
    types::{PyAnyMethods, PyTypeMethods},
};
use regex::Regex;

static RE_MULTI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"missing \d+ required positional arguments?: (.+)").unwrap());

static RE_SINGLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"missing 1 required positional argument: '([^']+)'").unwrap());

/// Extract missing arguments from a test function error.
///
/// If the error is of the form "missing 1 required positional argument: 'a'", return a set with "a".
///
/// If the error is of the form "missing 2 required positional arguments: 'a' and 'b'", return a set with "a" and "b".
pub(crate) fn missing_arguments_from_error(err: &str) -> HashSet<String> {
    RE_MULTI.captures(err).map_or_else(
        || {
            RE_SINGLE.captures(err).map_or_else(HashSet::new, |caps| {
                HashSet::from([caps.get(1).unwrap().as_str().to_string()])
            })
        },
        |caps| {
            let args_str = caps.get(1).unwrap().as_str();
            let args_str = args_str.replace(" and ", ", ");
            let mut result = HashSet::new();
            for part in args_str.split(',') {
                let trimmed = part.trim();
                if trimmed.len() > 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
                    result.insert(trimmed[1..trimmed.len() - 1].to_string());
                }
            }
            result
        },
    )
}

/// Check for instances of `pytest.ParameterSet` and extract the parameters
/// from it.
pub(super) fn handle_custom_fixture_params(py: Python, params: Vec<Py<PyAny>>) -> Vec<Py<PyAny>> {
    params
        .into_iter()
        .filter_map(|param| {
            let Ok(bound_param) = param.into_bound_py_any(py) else {
                return None;
            };

            let a_type = bound_param.get_type();

            let Ok(type_name) = a_type.name() else {
                return Some(bound_param.into_py_any(py).unwrap());
            };

            if !type_name.contains("ParameterSet").unwrap_or_default() {
                return Some(bound_param.into_py_any(py).unwrap());
            }

            let Ok(params) = bound_param.getattr("values") else {
                return Some(bound_param.into_py_any(py).unwrap());
            };

            let Ok(first_param) = params.get_item(0) else {
                return Some(bound_param.into_py_any(py).unwrap());
            };

            Some(first_param.into_py_any(py).unwrap())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_arguments_from_error() {
        let err = "missing 2 required positional arguments: 'a' and 'b'";
        let missing_args = missing_arguments_from_error(err);
        assert_eq!(
            missing_args,
            HashSet::from([String::from("a"), String::from("b")])
        );
    }

    #[test]
    fn test_missing_arguments_from_error_single() {
        let err = "missing 1 required positional argument: 'a'";
        let missing_args = missing_arguments_from_error(err);
        assert_eq!(missing_args, HashSet::from([String::from("a")]));
    }
}
