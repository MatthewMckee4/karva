use std::collections::HashMap;

use pyo3::{prelude::*, types::PyDict};

use crate::{
    IndividualTestResultKind, Reporter, TestRunResult,
    diagnostic::{Diagnostic, FunctionDefinitionLocation},
    discovery::{DiscoveredModule, TestFunction},
    extensions::{
        fixtures::{
            Finalizers, RequiresFixtures, handle_missing_fixtures, missing_arguments_from_error,
        },
        tags::python::SkipError,
    },
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

    /// The missing fixtures diagnostic.
    diagnostic: Option<Diagnostic>,

    /// The diagnostic from collecting the test case.
    ///
    /// These fixture diagnostics come from the collection process and are most likely
    /// diagnostics of failed fixtures.
    fixture_diagnostics: Vec<Diagnostic>,
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
            fixture_diagnostics: Vec::new(),
        }
    }

    pub(crate) fn add_finalizers(&mut self, finalizers: Finalizers) {
        self.finalizers.update(finalizers);
    }

    pub(crate) fn add_fixture_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) {
        self.fixture_diagnostics.extend(diagnostics);
    }

    pub(crate) fn run(self, py: Python<'_>, reporter: &dyn Reporter) -> TestRunResult {
        let Self {
            function,
            kwargs,
            module,
            finalizers,
            diagnostic,
            fixture_diagnostics,
        } = self;

        let mut run_result = (|| {
            let mut run_result = TestRunResult::default();

            let test_name = full_test_name(py, &function.name().to_string(), &kwargs);

            if let Some(skip_tag) = &function.tags().skip_tag() {
                run_result.register_test_case_result(
                    &test_name,
                    IndividualTestResultKind::Skipped {
                        reason: skip_tag.reason(),
                    },
                    Some(reporter),
                );

                return run_result;
            }

            if let Some(skip_if_tag) = &function.tags().skip_if_tag() {
                if skip_if_tag.should_skip() {
                    run_result.register_test_case_result(
                        &test_name,
                        IndividualTestResultKind::Skipped {
                            reason: skip_if_tag.reason(),
                        },
                        Some(reporter),
                    );
                    return run_result;
                }
            }

            let case_call_result = if kwargs.is_empty() {
                function.py_function().call0(py)
            } else {
                let py_dict = PyDict::new(py);

                for key in function.definition().required_fixtures(py) {
                    if let Some(value) = kwargs.get(&key) {
                        let _ = py_dict.set_item(key, value);
                    }
                }

                function.py_function().call(py, (), Some(&py_dict))
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

            let default_diagnostic = || {
                Diagnostic::from_test_fail(
                    py,
                    &err,
                    FunctionDefinitionLocation::new(
                        function.name().to_string(),
                        function.display_with_line(module),
                    ),
                )
            };

            let diagnostic = diagnostic.map_or_else(default_diagnostic, |input_diagnostic| {
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
        })();

        run_result.add_test_diagnostics(fixture_diagnostics);

        run_result.add_test_diagnostics(finalizers.run(py));

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

    false
}

/// Extract the skip reason from a skip exception.
fn extract_skip_reason(py: Python<'_>, err: &PyErr) -> Option<String> {
    let value = err.value(py);

    // Try to get the first argument (the message)
    if let Ok(args) = value.getattr("args")
        && let Ok(tuple) = args.cast::<pyo3::types::PyTuple>()
        && let Ok(first_arg) = tuple.get_item(0)
        && let Ok(message) = first_arg.extract::<String>()
    {
        if message.is_empty() {
            return None;
        }
        return Some(message);
    }

    None
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
