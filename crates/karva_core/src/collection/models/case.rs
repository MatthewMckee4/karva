use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

use pyo3::{prelude::*, types::PyDict};
use regex::Regex;

use crate::{
    Reporter,
    diagnostic::{Diagnostic, MissingFixturesDiagnostic, diagnostic::FunctionDefinitionLocation},
    discovery::{DiscoveredModule, TestFunction},
    extensions::{
        fixtures::{Finalizers, UsesFixtures},
        tags::python::SkipError,
    },
    runner::{TestRunResult, diagnostic::IndividualTestResultKind},
};

/// A test case represents a test function with a set of arguments.
#[derive(Debug)]
pub(crate) struct TestCase<'proj> {
    /// The test function to run.
    function: &'proj TestFunction,

    /// The arguments to pass to the test function.
    kwargs: HashMap<String, Py<PyAny>>,

    /// The module containing the test function.
    module: &'proj DiscoveredModule,

    /// Finalizers to run after the test case is executed.
    finalizers: Finalizers,

    /// The diagnostic from collecting the test case.
    diagnostic: Option<Diagnostic>,
}

impl<'proj> TestCase<'proj> {
    pub(crate) fn new(
        function: &'proj TestFunction,
        kwargs: HashMap<String, Py<PyAny>>,
        module: &'proj DiscoveredModule,
        diagnostic: Option<Diagnostic>,
    ) -> Self {
        Self {
            function,
            kwargs,
            module,
            finalizers: Finalizers::default(),
            diagnostic,
        }
    }

    pub(crate) const fn function(&self) -> &TestFunction {
        self.function
    }

    pub(crate) fn add_finalizers(&mut self, finalizers: Finalizers) {
        self.finalizers.update(finalizers);
    }

    pub(crate) const fn finalizers(&self) -> &Finalizers {
        &self.finalizers
    }

    pub(crate) fn run(&self, py: Python<'_>, reporter: &dyn Reporter) -> TestRunResult {
        let mut run_result = TestRunResult::default();

        let test_name = full_test_name(py, &self.function.name().to_string(), &self.kwargs);

        if let Some(skip_tag) = &self.function.tags().skip_tag() {
            run_result.register_test_case_result(
                &test_name,
                IndividualTestResultKind::Skipped {
                    reason: skip_tag.reason(),
                },
                Some(reporter),
            );

            return run_result;
        }

        let case_call_result = if self.kwargs.is_empty() {
            self.function.py_function().call0(py)
        } else {
            let kwargs = PyDict::new(py);

            for key in self.function.definition().dependant_fixtures(py) {
                if let Some(value) = self.kwargs.get(&key) {
                    let _ = kwargs.set_item(key, value);
                }
            }

            self.function.py_function().call(py, (), Some(&kwargs))
        };

        let Err(err) = case_call_result else {
            run_result.register_test_case_result(
                &test_name,
                IndividualTestResultKind::Passed,
                Some(reporter),
            );

            return run_result;
        };

        // Check if the exception is a skip exception (karva.SkipError or pytest.Skipped)
        if is_skip_exception(py, &err) {
            let reason = extract_skip_reason(py, &err);
            run_result.register_test_case_result(
                &test_name,
                IndividualTestResultKind::Skipped { reason },
                Some(reporter),
            );

            return run_result;
        }

        let default_diagnostic = || Diagnostic::from_test_fail(py, &err, self, self.module);

        let diagnostic =
            self.diagnostic
                .clone()
                .map_or_else(default_diagnostic, |input_diagnostic| {
                    let missing_args = missing_arguments_from_error(&err.to_string());
                    handle_missing_fixtures(&missing_args, input_diagnostic)
                        .unwrap_or_else(default_diagnostic)
                });

        run_result.register_test_case_result(
            &test_name,
            IndividualTestResultKind::Failed,
            Some(reporter),
        );

        run_result.add_test_diagnostic(diagnostic);

        run_result
    }
}

/// Check if the given `PyErr` is a skip exception (karva.SkipError or pytest.skip.Exception/Skipped).
fn is_skip_exception(py: Python<'_>, err: &PyErr) -> bool {
    // Check for karva.SkipError
    if err.is_instance_of::<SkipError>(py) {
        return true;
    }

    // Check for pytest.skip.Exception (the actual exception raised by pytest.skip())
    if let Ok(pytest_module) = py.import("_pytest.outcomes")
        && let Ok(skipped) = pytest_module.getattr("Skipped")
        && err.matches(py, skipped).unwrap_or(false)
    {
        return true;
    }

    // Also check the public pytest.skip namespace as a fallback
    if let Ok(pytest_module) = py.import("pytest")
        && let Ok(skip_module) = pytest_module.getattr("skip")
        && let Ok(exception) = skip_module.getattr("Exception")
        && err.matches(py, exception).unwrap_or(false)
    {
        return true;
    }

    false
}

/// Extract the skip reason from a skip exception.
fn extract_skip_reason(py: Python<'_>, err: &PyErr) -> Option<String> {
    let value = err.value(py);

    // Try to get the first argument (the message)
    if let Ok(args) = value.getattr("args")
        && let Ok(tuple) = args.cast::<pyo3::types::PyTuple>()
        && let Ok(first_arg) = tuple.get_item(0)
        && let Ok(msg) = first_arg.extract::<String>()
    {
        return Some(msg);
    }

    // Fallback to string representation of the exception
    if let Ok(msg) = value.str() {
        return Some(msg.to_string());
    }

    None
}

/// Handle missing fixtures.
///
/// If the diagnostic has a sub-diagnostic with a fixture not found error, and the missing fixture is in the set of missing arguments,
/// return the diagnostic with the sub-diagnostic removed.
///
/// Otherwise, return None.
fn handle_missing_fixtures(
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
fn missing_arguments_from_error(err: &str) -> HashSet<String> {
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

fn full_test_name(py: Python, function: &str, kwargs: &HashMap<String, Py<PyAny>>) -> String {
    if kwargs.is_empty() {
        function.to_string()
    } else {
        let mut args_str = String::new();
        let mut sorted_kwargs: Vec<_> = kwargs.iter().collect();
        sorted_kwargs.sort_by_key(|(key, _)| *key);

        for (i, (key, value)) in sorted_kwargs.iter().enumerate() {
            if i > 0 {
                args_str.push_str(", ");
            }
            if let Ok(value) = value.cast_bound::<PyAny>(py) {
                args_str.push_str(&format!("{key}={value:?}"));
            }
        }
        format!("{function} [{args_str}]")
    }
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
