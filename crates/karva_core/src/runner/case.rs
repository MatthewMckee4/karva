use std::collections::HashMap;

use pyo3::{prelude::*, types::PyDict};

use crate::{
    Context, IndividualTestResultKind,
    diagnostic::{Diagnostic, FunctionDefinitionLocation},
    discovery::{DiscoveredModule, TestFunction},
    extensions::{
        fixtures::{RequiresFixtures, missing_arguments_from_error},
        tags::{ExpectFailTag, python::SkipError},
    },
};

/// A test case represents a test function with a set of arguments.
#[derive(Debug)]
pub struct TestCase<'proj> {
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
