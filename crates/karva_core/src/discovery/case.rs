use std::{
    cmp::{Eq, PartialEq},
    fmt::{self, Display},
    hash::{Hash, Hasher},
    time::{Duration, Instant},
};

use pyo3::{exceptions::PyAssertionError, prelude::*};
use ruff_python_ast::StmtFunctionDef;

use crate::test_result::TestResult;

#[derive(Debug, Clone)]
pub struct TestCase {
    module: String,
    function_definition: StmtFunctionDef,
}

impl TestCase {
    #[must_use]
    pub const fn new(module: String, function_definition: StmtFunctionDef) -> Self {
        Self {
            module,
            function_definition,
        }
    }

    #[must_use]
    pub const fn module(&self) -> &String {
        &self.module
    }

    #[must_use]
    pub const fn function_definition(&self) -> &StmtFunctionDef {
        &self.function_definition
    }

    #[must_use]
    pub fn run_test(&self, py: Python, imported_module: &Bound<'_, PyModule>) -> TestResult {
        let start_time = Instant::now();

        let function = match imported_module.getattr(self.function_definition().name.to_string()) {
            Ok(function) => function,
            Err(e) => {
                return TestResult::new_error(self.clone(), e.to_string(), start_time.elapsed());
            }
        };

        let result = function.call0();
        let duration = start_time.elapsed();

        match result {
            Ok(_) => TestResult::new_pass(self.clone(), duration),
            Err(err) => self.handle_run_error(py, &err, duration),
        }
    }

    fn handle_run_error(&self, py: Python, error: &PyErr, duration: Duration) -> TestResult {
        let err_value = error.value(py);
        if err_value.is_instance_of::<PyAssertionError>() {
            let traceback = error
                .traceback(py)
                .map(|traceback| filter_traceback(&traceback.format().unwrap_or_default()));
            TestResult::new_fail(self.clone(), traceback, duration)
        } else {
            let traceback = error
                .traceback(py)
                .map(|traceback| filter_traceback(&traceback.format().unwrap_or_default()))
                .unwrap_or_default();
            TestResult::new_error(self.clone(), traceback, duration)
        }
    }
}

impl Display for TestCase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.module, self.function_definition.name)
    }
}

impl Hash for TestCase {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.module.hash(state);
        self.function_definition.name.hash(state);
    }
}

impl PartialEq for TestCase {
    fn eq(&self, other: &Self) -> bool {
        self.module == other.module
            && self.function_definition.name == other.function_definition.name
    }
}

impl Eq for TestCase {}

fn filter_traceback(traceback: &str) -> String {
    let lines: Vec<&str> = traceback.lines().collect();
    let mut filtered = String::new();

    for (i, line) in lines.iter().enumerate() {
        if i == 0 && line.contains("Traceback (most recent call last):") {
            continue;
        }
        if line.starts_with("  ") {
            if let Some(stripped) = line.strip_prefix("  ") {
                filtered.push_str(stripped);
            }
        } else {
            filtered.push_str(line);
        }
        filtered.push('\n');
    }

    filtered.trim_end().to_string()
}
