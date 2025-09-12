use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

use pyo3::{prelude::*, types::PyDict};
use regex::Regex;

use crate::{
    diagnostic::{
        Diagnostic, FixtureSubDiagnosticType, SubDiagnosticErrorType, SubDiagnosticSeverity,
    },
    discovery::{DiscoveredModule, TestFunction, TestFunctionDisplay},
    extensions::fixtures::{Finalizers, UsesFixtures},
    runner::RunDiagnostics,
};

/// A test case represents a single test function invocation with a set of arguments.
#[derive(Debug)]
pub(crate) struct TestCase<'proj> {
    /// The test function to run.
    function: &'proj TestFunction,

    /// The arguments to pass to the test function.
    kwargs: HashMap<String, PyObject>,

    /// The Python function to call.
    py_function: Py<PyAny>,

    /// The module containing the test function.
    module: &'proj DiscoveredModule,

    /// Finalizers to run after the test case is executed.
    finalizers: Finalizers,
}

impl<'proj> TestCase<'proj> {
    pub(crate) fn new(
        function: &'proj TestFunction,
        kwargs: HashMap<String, PyObject>,
        py_function: Py<PyAny>,
        module: &'proj DiscoveredModule,
    ) -> Self {
        Self {
            function,
            kwargs,
            py_function,
            module,
            finalizers: Finalizers::default(),
        }
    }

    #[must_use]
    pub(crate) const fn function(&self) -> &TestFunction {
        self.function
    }

    pub(crate) fn add_finalizers(&mut self, finalizers: Finalizers) {
        self.finalizers.update(finalizers);
    }

    #[must_use]
    pub(crate) const fn finalizers(&self) -> &Finalizers {
        &self.finalizers
    }

    #[must_use]
    pub(crate) fn run(&self, py: Python<'_>, diagnostic: Option<Diagnostic>) -> RunDiagnostics {
        let mut run_result = RunDiagnostics::default();

        let display = self.function.display();

        let (case_call_result, logger) = if self.kwargs.is_empty() {
            let logger = TestCaseLogger::new(py, &display, None);
            logger.log_running();
            (self.py_function.call0(py), logger)
        } else {
            let kwargs = PyDict::new(py);

            for key in self.function.definition().dependant_fixtures(py) {
                if let Some(value) = self.kwargs.get(&key) {
                    let _ = kwargs.set_item(key, value);
                }
            }
            let logger = TestCaseLogger::new(py, &display, Some(&self.kwargs));
            logger.log_running();
            (self.py_function.call(py, (), Some(&kwargs)), logger)
        };

        match case_call_result {
            Ok(_) => {
                logger.log_passed();
                run_result.stats_mut().add_passed();
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
                    logger.log_failed();
                    run_result.stats_mut().add_failed();
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

/// Log the status of a test case.
struct TestCaseLogger {
    test_name: String,
}

impl TestCaseLogger {
    #[must_use]
    fn new(
        py: Python<'_>,
        function: &TestFunctionDisplay<'_>,
        kwargs: Option<&HashMap<String, PyObject>>,
    ) -> Self {
        let test_name = kwargs.map_or_else(
            || function.to_string(),
            |kwargs| {
                let mut args_str = String::new();
                for (i, (key, value)) in kwargs.iter().enumerate() {
                    if i > 0 {
                        args_str.push_str(", ");
                    }
                    if let Ok(value) = value.downcast_bound::<PyAny>(py) {
                        args_str.push_str(&format!("{key}={value:?}"));
                    }
                }
                format!("{function} [{args_str}]")
            },
        );

        Self { test_name }
    }

    fn log(&self, status: &str) {
        tracing::info!("{:<8} | {}", status, self.test_name);
    }

    fn log_running(&self) {
        self.log("running");
    }

    fn log_passed(&self) {
        self.log("passed");
    }

    fn log_failed(&self) {
        self.log("failed");
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
