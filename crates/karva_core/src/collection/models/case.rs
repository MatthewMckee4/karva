use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

use pyo3::{prelude::*, types::PyDict};
use regex::Regex;

use crate::{
    Reporter,
    diagnostic::{
        Diagnostic, FixtureSubDiagnosticType, SubDiagnosticErrorType, SubDiagnosticSeverity,
    },
    discovery::{DiscoveredModule, TestFunction},
    extensions::{
        fixtures::{Finalizers, UsesFixtures},
        tags::SkipTag,
    },
    runner::{TestRunResult, diagnostic::IndividualTestResultKind},
};

/// A test case represents a single test function invocation with a set of arguments.
#[derive(Debug)]
pub(crate) struct TestCase<'proj> {
    /// The test function to run.
    function: &'proj TestFunction,

    /// The arguments to pass to the test function.
    kwargs: HashMap<String, Py<PyAny>>,

    /// The Python function to call.
    py_function: Py<PyAny>,

    /// The module containing the test function.
    module: &'proj DiscoveredModule,

    /// Finalizers to run after the test case is executed.
    finalizers: Finalizers,

    skip: Option<SkipTag>,
}

impl<'proj> TestCase<'proj> {
    pub(crate) fn new(
        function: &'proj TestFunction,
        kwargs: HashMap<String, Py<PyAny>>,
        py_function: Py<PyAny>,
        module: &'proj DiscoveredModule,
        skip: Option<SkipTag>,
    ) -> Self {
        Self {
            function,
            kwargs,
            py_function,
            module,
            finalizers: Finalizers::default(),
            skip,
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

    pub(crate) fn run(
        &self,
        py: Python<'_>,
        diagnostic: Option<Diagnostic>,
        reporter: &dyn Reporter,
    ) -> TestRunResult {
        let mut run_result = TestRunResult::default();

        let display = self.function.display();

        let (case_call_result, test_name) = if self.kwargs.is_empty() {
            let test_name = full_test_name(py, &display.to_string(), None);

            if let Some(skip_tag) = &self.skip {
                run_result.register_test_case_result(
                    &test_name,
                    IndividualTestResultKind::Skipped {
                        reason: skip_tag.reason(),
                    },
                    Some(reporter),
                );

                return run_result;
            }

            (self.py_function.call0(py), test_name)
        } else {
            let kwargs = PyDict::new(py);

            let test_name = full_test_name(py, &display.to_string(), Some(&self.kwargs));

            for key in self.function.definition().dependant_fixtures(py) {
                if let Some(value) = self.kwargs.get(&key) {
                    let _ = kwargs.set_item(key, value);
                }
            }

            if let Some(skip_tag) = &self.skip {
                run_result.register_test_case_result(
                    &test_name,
                    IndividualTestResultKind::Skipped {
                        reason: skip_tag.reason(),
                    },
                    Some(reporter),
                );

                return run_result;
            }

            (self.py_function.call(py, (), Some(&kwargs)), test_name)
        };

        match case_call_result {
            Ok(_) => {
                run_result.register_test_case_result(
                    &test_name,
                    IndividualTestResultKind::Passed,
                    Some(reporter),
                );
            }
            Err(err) => {
                let diagnostic = diagnostic.map_or_else(
                    || Diagnostic::from_test_fail(py, &err, self, self.module),
                    |input_diagnostic| {
                        let missing_args = missing_arguments_from_error(&err.to_string());
                        handle_missing_fixtures(&missing_args, input_diagnostic).unwrap_or_else(
                            || Diagnostic::from_test_fail(py, &err, self, self.module),
                        )
                    },
                );

                let error_type = diagnostic.severity();

                if error_type.is_test_fail() {
                    run_result.register_test_case_result(
                        &test_name,
                        IndividualTestResultKind::Failed,
                        Some(reporter),
                    );
                }

                run_result.add_diagnostic(diagnostic);
            }
        }

        run_result
    }
}

/// Handle missing fixtures.
///
/// If the diagnostic has a sub-diagnostic with a fixture not found error, and the missing fixture is in the set of missing arguments,
/// return the diagnostic with the sub-diagnostic removed.
///
/// Otherwise, return None.
fn handle_missing_fixtures(
    missing_args: &HashSet<String>,
    mut diagnostic: Diagnostic,
) -> Option<Diagnostic> {
    let sub_diagnostics: Vec<_> = diagnostic
        .sub_diagnostics()
        .iter()
        .filter_map(|sd| {
            let SubDiagnosticSeverity::Error(SubDiagnosticErrorType::Fixture(
                FixtureSubDiagnosticType::NotFound(fixture_name),
            )) = sd.severity();

            if missing_args.contains(fixture_name) {
                Some(sd.clone())
            } else {
                None
            }
        })
        .collect();

    diagnostic.clear_sub_diagnostics();
    if sub_diagnostics.is_empty() {
        None
    } else {
        diagnostic.add_sub_diagnostics(sub_diagnostics);
        Some(diagnostic)
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

fn full_test_name(
    py: Python,
    function: &str,
    kwargs: Option<&HashMap<String, Py<PyAny>>>,
) -> String {
    kwargs.map_or_else(
        || function.to_string(),
        |kwargs| {
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
