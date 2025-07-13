use std::fmt::{self, Display};

use karva_project::path::SystemPathBuf;
use pyo3::{prelude::*, types::PyTuple};

use crate::{
    diagnostic::Diagnostic,
    discovery::{DiscoveredModule, TestFunction, TestFunctionDisplay},
    extensions::fixtures::Finalizers,
    runner::RunDiagnostics,
};

#[derive(Debug)]
pub struct TestCase<'proj> {
    function: &'proj TestFunction<'proj>,
    args: Vec<PyObject>,
    py_function: Py<PyAny>,
    module: &'proj DiscoveredModule<'proj>,
    finalizers: Finalizers,
}

impl<'proj> TestCase<'proj> {
    pub fn new(
        function: &'proj TestFunction<'proj>,
        args: Vec<PyObject>,
        py_function: Py<PyAny>,
        module: &'proj DiscoveredModule<'proj>,
    ) -> Self {
        Self {
            function,
            args,
            py_function,
            module,
            finalizers: Finalizers::default(),
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
    pub fn run(&self, py: Python<'_>) -> RunDiagnostics {
        let mut run_result = RunDiagnostics::default();

        if self.args.is_empty() {
            match self.py_function.call0(py) {
                Ok(_) => {
                    run_result.stats_mut().add_passed();
                }
                Err(err) => {
                    run_result.add_diagnostic(Diagnostic::from_test_fail(
                        py,
                        &err,
                        self,
                        self.module,
                    ));
                }
            }
        } else {
            let test_function_arguments = PyTuple::new(py, self.args.clone());

            match test_function_arguments {
                Ok(args) => {
                    let display = self
                        .function
                        .display(self.module.path().display().to_string());
                    let logger = TestCaseLogger::new(&display, args.clone());
                    logger.log_running();
                    match self.py_function.call1(py, args) {
                        Ok(_) => {
                            logger.log_passed();
                            run_result.stats_mut().add_passed();
                        }
                        Err(err) => {
                            let diagnostic =
                                Diagnostic::from_test_fail(py, &err, self, self.module);
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
                        Some(err.to_string()),
                        Some(self.function.display_with_line(self.module)),
                    ));
                }
            }
        }

        run_result
    }
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
