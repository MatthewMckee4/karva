use pyo3::{prelude::*, types::PyTuple};

use crate::{
    diagnostic::{Diagnostic, ErrorType, Severity},
    models::{TestFunction, test_function::TestFunctionDisplay},
    runner::RunDiagnostics,
};

#[derive(Debug)]
pub struct TestCase<'proj> {
    function: &'proj TestFunction<'proj>,
    args: Vec<PyObject>,
    py_module: Py<PyModule>,
}

impl<'proj> TestCase<'proj> {
    pub const fn new(
        function: &'proj TestFunction<'proj>,
        args: Vec<PyObject>,
        py_module: Py<PyModule>,
    ) -> Self {
        Self {
            function,
            args,
            py_module,
        }
    }

    #[must_use]
    pub const fn function(&self) -> &TestFunction<'proj> {
        self.function
    }

    #[must_use]
    pub fn args(&self) -> &[PyObject] {
        &self.args
    }

    #[must_use]
    pub fn run(&self, py: Python<'_>) -> RunDiagnostics {
        let mut run_result = RunDiagnostics::default();

        let py_function = match self.py_module.getattr(py, self.function.name()) {
            Ok(function) => function,
            Err(err) => {
                run_result.add_diagnostic(Diagnostic::from_py_err(
                    py,
                    &err,
                    Some(self.function.name()),
                    Severity::Error(ErrorType::Unknown),
                ));
                return run_result;
            }
        };

        if self.args.is_empty() {
            match py_function.call0(py) {
                Ok(_) => {
                    run_result.stats_mut().add_passed();
                }
                Err(err) => {
                    run_result.add_diagnostic(Diagnostic::from_test_fail(py, &err, self));
                }
            }
        } else {
            let test_function_arguments = PyTuple::new(py, self.args.clone());

            match test_function_arguments {
                Ok(args) => {
                    let display = self.function.display(self.function.module_name());
                    let logger = TestCaseLogger::new(&display, args.clone());
                    logger.log_running();
                    match py_function.call1(py, args) {
                        Ok(_) => {
                            logger.log_passed();
                            run_result.stats_mut().add_passed();
                        }
                        Err(err) => {
                            let diagnostic = Diagnostic::from_test_fail(py, &err, self);
                            let error_type = diagnostic.severity();
                            if error_type.is_test_fail() {
                                logger.log_failed();
                            } else if error_type.is_test_error() {
                                logger.log_errored();
                            }
                            run_result.add_diagnostic(diagnostic);
                        }
                    }
                }
                Err(err) => {
                    run_result.add_diagnostic(Diagnostic::unknown_error(
                        err.to_string(),
                        Some(
                            self.function
                                .display(self.function.module_name())
                                .to_string(),
                        ),
                    ));
                }
            }
        }

        run_result
    }
}

struct TestCaseLogger<'a> {
    function: &'a TestFunctionDisplay<'a>,
    args: Bound<'a, PyTuple>,
}

impl<'a> TestCaseLogger<'a> {
    #[must_use]
    const fn new(function: &'a TestFunctionDisplay<'a>, args: Bound<'a, PyTuple>) -> Self {
        Self { function, args }
    }

    #[must_use]
    fn test_name(&self) -> String {
        if self.args.is_empty() {
            self.function.to_string()
        } else {
            let args_str = self
                .args
                .iter()
                .map(|a| format!("{a:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} [{args_str}]", self.function)
        }
    }

    fn log(&self, status: &str) {
        tracing::info!("{:<8} | {}", status, self.test_name());
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
