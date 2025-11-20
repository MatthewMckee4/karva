use std::collections::HashMap;

use pyo3::{prelude::*, types::PyDict};

use crate::{
    Context, IndividualTestResultKind,
    diagnostic::{Diagnostic, FunctionDefinitionLocation},
    discovery::{DiscoveredModule, TestFunction},
    extensions::{
        fixtures::{
            RequiresFixtures, handle_missing_fixtures, missing_arguments_from_error,
        },
        tags::{ExpectFailTag, python::SkipError},
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

    /// The missing fixtures diagnostic.
    diagnostic: Option<Diagnostic>,
}

impl<'proj> TestCase<'proj> {
    pub(crate) const fn new(
        function: &'proj TestFunction,
        kwargs: HashMap<String, Py<PyAny>>,
        module: &'proj DiscoveredModule,
        diagnostic: Option<Diagnostic>,
    ) -> Self {
        Self {
            function,
            kwargs,
            module,
            diagnostic,
        }
    }

    /// Runs the test case.
    ///
    /// Returns `true` if the test case passed or skipped, `false` otherwise.
    pub(crate) fn run(self, py: Python<'_>, context: &mut Context) -> bool {
        let Self {
            function,
            kwargs,
            module,
            diagnostic,
        } = self;

        let reporter = context.reporter();
        let run_result = context.result_mut();



        (|| {
            let test_name = full_test_name(py, &function.name().to_string(), &kwargs);

            // Check if test should be skipped
            if let Some(skip_tag) = &function.tags().skip_tag() {
                if skip_tag.should_skip() {
                    run_result.register_test_case_result(
                        &test_name,
                        IndividualTestResultKind::Skipped {
                            reason: skip_tag.reason(),
                        },
                        Some(reporter),
                    );
                    return true;
                }
            }

            // Check if test is expected to fail
            let expect_fail_tag = function.tags().expect_fail_tag();
            let expect_fail = expect_fail_tag
                .as_ref()
                .is_some_and(ExpectFailTag::should_expect_fail);

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
                if expect_fail {
                    // Test was expected to fail but passed - report as failed (unexpected pass)

                    let reason = expect_fail_tag.and_then(|tag| tag.reason());
                    let diagnostic = Diagnostic::pass_on_expect_fail(
                        reason,
                        FunctionDefinitionLocation::new(
                            function.name().to_string(),
                            function.display_with_line(module),
                        ),
                    );

                    run_result.register_test_case_result(
                        &test_name,
                        IndividualTestResultKind::Failed,
                        Some(reporter),
                    );
                    run_result.add_test_diagnostic(diagnostic);

                    return false;
                }

                run_result.register_test_case_result(
                    &test_name,
                    IndividualTestResultKind::Passed,
                    Some(reporter),
                );

                return true;
            };

            // Check if the exception is a skip exception
            if is_skip_exception(py, &err) {
                let reason = extract_skip_reason(py, &err);

                run_result.register_test_case_result(
                    &test_name,
                    IndividualTestResultKind::Skipped { reason },
                    Some(reporter),
                );

                return true;
            }

            // Test was expected to fail and did fail - report as passed (expected failure)
            if expect_fail {
                run_result.register_test_case_result(
                    &test_name,
                    IndividualTestResultKind::Passed,
                    Some(reporter),
                );

                return true;
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

            false
        })()
    }
}

/// Check if the given `PyErr` is a skip exception.
fn is_skip_exception(py: Python<'_>, err: &PyErr) -> bool {
    // Check for karva.SkipError
    if err.is_instance_of::<SkipError>(py) {
        return true;
    }

    // Check for pytest skip exception (the actual exception raised by pytest.skip())
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
