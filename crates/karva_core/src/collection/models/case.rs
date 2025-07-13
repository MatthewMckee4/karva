use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Display},
};

use karva_project::path::SystemPathBuf;
use pyo3::{prelude::*, types::PyDict};
use regex::Regex;

use crate::{
    diagnostic::{
        Diagnostic, FixtureSubDiagnosticType, SubDiagnosticErrorType, SubDiagnosticSeverity,
    },
    discovery::{DiscoveredModule, TestFunction, TestFunctionDisplay},
    extensions::fixtures::Finalizers,
    runner::RunDiagnostics,
};

#[derive(Debug)]
pub struct TestCase<'proj> {
    function: &'proj TestFunction<'proj>,
    kwargs: HashMap<String, PyObject>,
    py_function: Py<PyAny>,
    module: &'proj DiscoveredModule<'proj>,
    finalizers: Finalizers,
}

impl<'proj> TestCase<'proj> {
    pub fn new(
        function: &'proj TestFunction<'proj>,
        kwargs: HashMap<String, PyObject>,
        py_function: Py<PyAny>,
        module: &'proj DiscoveredModule<'proj>,
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
    pub const fn function(&self) -> &TestFunction<'proj> {
        self.function
    }

    pub fn add_finalizers(&mut self, finalizers: Finalizers) {
        self.finalizers.update(finalizers);
    }

    #[must_use]
    pub const fn finalizers(&self) -> &Finalizers {
        &self.finalizers
    }

    #[must_use]
    pub fn display(&self) -> TestCaseDisplay<'_> {
        TestCaseDisplay {
            test_case: self,
            module_path: self.module.path().clone(),
        }
    }

    #[must_use]
    pub fn run(&self, py: Python<'_>, diagnostic: Option<Diagnostic>) -> RunDiagnostics {
        let mut run_result = RunDiagnostics::default();

        let handle_missing_fixtures = |missing_args: &HashSet<String>| -> Option<Diagnostic> {
            diagnostic.and_then(|mut diagnostic| {
                let sub_diagnostics = diagnostic
                    .sub_diagnostics()
                    .iter()
                    .filter_map(|sd| {
                        if let SubDiagnosticSeverity::Error(SubDiagnosticErrorType::Fixture(
                            FixtureSubDiagnosticType::NotFound(fixture_name),
                        )) = sd.severity()
                        {
                            if missing_args.contains(fixture_name) {
                                Some(sd.clone())
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                diagnostic.clear_sub_diagnostics();
                if sub_diagnostics.is_empty() {
                    None
                } else {
                    diagnostic.add_sub_diagnostics(sub_diagnostics);
                    Some(diagnostic)
                }
            })
        };

        let kwargs = PyDict::new(py);

        for (key, value) in &self.kwargs {
            let _ = kwargs.set_item(key, value);
        }

        let display = self
            .function
            .display(self.module.path().display().to_string());
        let logger = TestCaseLogger::new(&display, &kwargs);
        logger.log_running();

        let case_call_result = if self.kwargs.is_empty() {
            self.py_function.call0(py)
        } else {
            self.py_function.call(py, (), Some(&kwargs))
        };

        match case_call_result {
            Ok(_) => {
                logger.log_passed();
                run_result.stats_mut().add_passed();
            }
            Err(err) => {
                let missing_args = missing_arguments_from_error(&err.to_string());

                let diagnostic = handle_missing_fixtures(&missing_args).map_or_else(
                    || Diagnostic::from_test_fail(py, &err, self, self.module),
                    |missing_fixtures| missing_fixtures,
                );

                let error_type = diagnostic.severity();

                if error_type.is_test_fail() {
                    logger.log_failed();
                    run_result.stats_mut().add_failed();
                } else if error_type.is_test_error() {
                    logger.log_errored();
                    run_result.stats_mut().add_errored();
                }

                run_result.add_diagnostic(diagnostic);
            }
        }

        run_result
    }
}

fn missing_arguments_from_error(err: &str) -> HashSet<String> {
    // Regex for "missing N required positional arguments: 'a' and 'b'"
    let re_multi = Regex::new(r"missing \d+ required positional arguments?: (.+)").unwrap();
    // Regex for "missing 1 required positional argument: 'a'"
    let re_single = Regex::new(r"missing 1 required positional argument: '([^']+)'").unwrap();

    if let Some(caps) = re_multi.captures(err) {
        // The group is something like: "'y' and 'z'" or "'x', 'y', and 'z'"
        let args_str = caps.get(1).unwrap().as_str();
        let args_str = args_str.replace(" and ", ", ");
        let mut result = HashSet::new();
        for part in args_str.split(',') {
            let trimmed = part.trim();
            if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() > 2 {
                result.insert(trimmed[1..trimmed.len() - 1].to_string());
            }
        }
        return result;
    } else if let Some(caps) = re_single.captures(err) {
        return HashSet::from([caps.get(1).unwrap().as_str().to_string()]);
    }
    HashSet::new()
}

pub struct TestCaseDisplay<'proj> {
    test_case: &'proj TestCase<'proj>,
    module_path: SystemPathBuf,
}

impl Display for TestCaseDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}::{}",
            self.module_path.display(),
            self.test_case.function().name()
        )
    }
}

struct TestCaseLogger {
    test_name: String,
}

impl TestCaseLogger {
    #[must_use]
    fn new(function: &TestFunctionDisplay<'_>, kwargs: &Bound<'_, PyDict>) -> Self {
        let test_name = if kwargs.is_empty() {
            function.to_string()
        } else {
            let args_str = kwargs
                .iter()
                .map(|(key, value)| format!("{key}={value:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{function} [{args_str}]",)
        };

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

    fn log_errored(&self) {
        self.log("errored");
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
