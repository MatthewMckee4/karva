use std::sync::LazyLock;

use regex::Regex;

static MISSING_POSITIONAL_ARGS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"missing \d+ required positional arguments?: (?P<args>.+)")
        .expect("missing-arguments regex is valid")
});

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

    let Some(args) = MISSING_POSITIONAL_ARGS
        .captures(err)
        .and_then(|captures| captures.name("args"))
    else {
        return Vec::new();
    };

    parse_quoted_argument_list(args.as_str())
}

fn parse_quoted_argument_list(arguments: &str) -> Vec<String> {
    arguments
        .replace(" and ", ", ")
        .split(',')
        .filter_map(|part| {
            let argument = part.trim().strip_prefix('\'')?.strip_suffix('\'')?;
            (!argument.is_empty()).then(|| argument.to_string())
        })
        .collect()
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
    fn test_missing_arguments_from_comma_list() {
        let err = "test_func() missing 3 required positional arguments: 'a', 'b', and 'c'";
        let missing_args = missing_arguments_from_error("test_func", err);
        assert_eq!(
            missing_args,
            vec![String::from("a"), String::from("b"), String::from("c")]
        );
    }

    #[test]
    fn test_missing_arguments_from_different_function() {
        let err = "test_func() missing 1 required positional argument: 'a'";
        let missing_args = missing_arguments_from_error("test_funca", err);
        assert!(missing_args.is_empty());
    }

    #[test]
    fn test_missing_arguments_from_unrecognized_message() {
        let err = "test_func() missing required keyword-only argument: 'a'";
        let missing_args = missing_arguments_from_error("test_func", err);
        assert!(missing_args.is_empty());
    }
}
