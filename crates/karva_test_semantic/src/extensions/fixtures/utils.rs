use std::sync::LazyLock;

use regex::Regex;

static RE_MULTI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"missing \d+ required positional arguments?: (.+)").unwrap());

static RE_SINGLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"missing 1 required positional argument: '([^']+)'").unwrap());

/// Extract missing arguments from a test function error.
///
/// If the error is of the form "missing 1 required positional argument: 'a'", return a set with "a".
/// If the error is of the form "missing 2 required positional arguments: 'a' and 'b'", return a set with "a" and "b".
///
/// We take the test name to ensure we don't provide argument names for inner functions. Only the function we expect.
pub fn missing_arguments_from_error(test_name: &str, err: &str) -> Vec<String> {
    if !err.contains(&format!("{test_name}()")) {
        return Vec::new();
    }

    RE_MULTI.captures(err).map_or_else(
        || {
            RE_SINGLE.captures(err).map_or_else(Vec::new, |caps| {
                vec![caps.get(1).unwrap().as_str().to_string()]
            })
        },
        |caps| {
            let args_str = caps.get(1).unwrap().as_str();
            let args_str = args_str.replace(" and ", ", ");
            let mut result = Vec::new();
            for part in args_str.split(',') {
                let trimmed = part.trim();
                if trimmed.len() > 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
                    result.push(trimmed[1..trimmed.len() - 1].to_string());
                }
            }
            result
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_arguments_from_error() {
        let err = "test_func() missing 2 required positional arguments: 'a' and 'b'";
        let missing_args = missing_arguments_from_error("test_func", err);
        assert_eq!(missing_args, vec![String::from("a"), String::from("b")]);
    }

    #[test]
    fn test_missing_arguments_from_error_single() {
        let err = "test_func() missing 1 required positional argument: 'a'";
        let missing_args = missing_arguments_from_error("test_func", err);
        assert_eq!(missing_args, vec![String::from("a")]);
    }

    #[test]
    fn test_missing_arguments_from_different_function() {
        let err = "test_func() missing 1 required positional argument: 'a'";
        let missing_args = missing_arguments_from_error("test_funca", err);
        assert!(missing_args.is_empty());
    }
}
