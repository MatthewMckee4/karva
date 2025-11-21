use std::collections::HashMap;

use pyo3::prelude::*;

/// Stringifies a Python parameter value for use in test/fixture names.
/// Uses Python's `repr()` for consistent stringification.
pub fn stringify_param(py: Python<'_>, param: &Py<PyAny>) -> String {
    // Use Python's repr() for consistent stringification
    match param.bind(py).repr() {
        Ok(repr) => repr.to_string(),
        Err(_) => {
            // Fallback to string conversion if repr fails
            match param.bind(py).str() {
                Ok(s) => s.to_string(),
                Err(_) => "<unprintable>".to_string(),
            }
        }
    }
}

/// Creates a parameter name string from a map of parameters.
/// Format: "param1=value1,param2=value2"
pub fn stringify_params(py: Python<'_>, params: &HashMap<String, Py<PyAny>>) -> String {
    if params.is_empty() {
        return String::new();
    }

    let mut sorted_keys: Vec<_> = params.keys().collect();
    sorted_keys.sort();

    sorted_keys
        .into_iter()
        .map(|key| {
            let value = &params[key];
            format!("{}={}", key, stringify_param(py, value))
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Computes the cartesian product of multiple vectors.
/// For example: `cartesian_product(vec`![vec![1,2], vec![3,4]])
/// returns vec![vec![1,3], vec![1,4], vec![2,3], vec![2,4]]
pub fn cartesian_product<T: Clone>(vecs: Vec<Vec<T>>) -> Vec<Vec<T>> {
    if vecs.is_empty() {
        return vec![vec![]];
    }

    let mut result = vec![vec![]];

    for vec in vecs {
        let mut new_result = Vec::new();
        for existing in &result {
            for item in &vec {
                let mut new_combination = existing.clone();
                new_combination.push(item.clone());
                new_result.push(new_combination);
            }
        }
        result = new_result;
    }

    result
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_cartesian_product_empty() {
//         let result: Vec<Vec<i32>> = cartesian_product(vec![]);
//         assert_eq!(result, vec![vec![]]);
//     }

//     #[test]
//     fn test_cartesian_product_single() {
//         let result = cartesian_product(vec![vec![1, 2, 3]]);
//         assert_eq!(result, vec![vec![1], vec![2], vec![3]]);
//     }

//     #[test]
//     fn test_cartesian_product_two() {
//         let result = cartesian_product(vec![vec![1, 2], vec![3, 4]]);
//         assert_eq!(
//             result,
//             vec![vec![1, 3], vec![1, 4], vec![2, 3], vec![2, 4]]
//         );
//     }

//     #[test]
//     fn test_cartesian_product_three() {
//         let result = cartesian_product(vec![vec![1, 2], vec![3], vec![4, 5]]);
//         assert_eq!(
//             result,
//             vec![
//                 vec![1, 3, 4],
//                 vec![1, 3, 5],
//                 vec![2, 3, 4],
//                 vec![2, 3, 5]
//             ]
//         );
//     }

//     #[test]
//     fn test_stringify_param() {
//         Python::with_gil(|py| {
//             // Test integer
//             let int_param = 42.into_pyobject(py).unwrap().unbind();
//             assert_eq!(stringify_param(py, &int_param), "42");

//             // Test string
//             let str_param = "hello".into_pyobject(py).unwrap().unbind();
//             assert_eq!(stringify_param(py, &str_param), "'hello'");

//             // Test boolean
//             let bool_param = true.into_pyobject(py).unwrap().unbind();
//             assert_eq!(stringify_param(py, &bool_param), "True");
//         });
//     }

//     #[test]
//     fn test_stringify_params() {
//         Python::with_gil(|py| {
//             let mut params = HashMap::new();
//             params.insert(
//                 "x".to_string(),
//                 1.into_pyobject(py).unwrap().unbind(),
//             );
//             params.insert(
//                 "y".to_string(),
//                 "a".into_pyobject(py).unwrap().unbind(),
//             );

//             let result = stringify_params(py, &params);
//             // Keys should be sorted alphabetically
//             assert_eq!(result, "x=1,y='a'");
//         });
//     }

//     #[test]
//     fn test_stringify_params_empty() {
//         Python::with_gil(|py| {
//             let params = HashMap::new();
//             let result = stringify_params(py, &params);
//             assert_eq!(result, "");
//         });
//     }
// }
