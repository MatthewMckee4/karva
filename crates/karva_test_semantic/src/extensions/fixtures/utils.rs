/// Extract missing arguments from a test function error.
///
/// If the error is of the form "missing 1 required positional argument: 'a'", return a set with "a".
/// If the error is of the form "missing 2 required positional arguments: 'a' and 'b'", return a set with "a" and "b".
///
/// We take the test name to ensure we don't provide argument names for inner functions. Only the function we expect.
pub fn missing_arguments_from_error(test_name: &str, error: &str) -> Vec<String> {
    let function_error = format!("{test_name}() missing ");
    let Some((_, message)) = error.split_once(&function_error) else {
        return Vec::new();
    };
    let Some((count, message)) = message.split_once(" required positional argument") else {
        return Vec::new();
    };
    let Some(arguments) = message
        .strip_prefix(": ")
        .or_else(|| message.strip_prefix("s: "))
    else {
        return Vec::new();
    };
    if count.parse::<usize>().is_err() {
        return Vec::new();
    }

    parse_quoted_argument_list(arguments)
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
