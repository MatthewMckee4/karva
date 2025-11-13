use std::{collections::HashSet, sync::LazyLock};

use regex::Regex;

use crate::diagnostic::{Diagnostic, FunctionDefinitionLocation, MissingFixturesDiagnostic};

/// Handle missing fixtures.
///
/// If the diagnostic has a sub-diagnostic with a fixture not found error, and the missing fixture is in the set of missing arguments,
/// return the diagnostic with the sub-diagnostic removed.
///
/// Otherwise, return None.
pub(crate) fn handle_missing_fixtures(
    missing_args: &HashSet<String>,
    diagnostic: Diagnostic,
) -> Option<Diagnostic> {
    let missing_fixtures_diagnostic = diagnostic.into_missing_fixtures()?;

    let MissingFixturesDiagnostic {
        location:
            FunctionDefinitionLocation {
                function_name,
                location,
            },
        missing_fixtures,
        function_kind,
    } = missing_fixtures_diagnostic;

    let actually_missing_fixtures = missing_fixtures
        .iter()
        .filter(|fixture| missing_args.contains(*fixture))
        .cloned()
        .collect::<Vec<_>>();

    if actually_missing_fixtures.is_empty() {
        None
    } else {
        Some(Diagnostic::missing_fixtures(
            actually_missing_fixtures,
            location,
            function_name,
            function_kind,
        ))
    }
}

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
