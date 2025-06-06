use std::{
    cmp::{Eq, PartialEq},
    fmt::{self, Display},
    hash::{Hash, Hasher},
};

use karva_project::{path::SystemPathBuf, utils::module_name};
use pyo3::{prelude::*, types::PyTuple};
use ruff_python_ast::StmtFunctionDef;

use crate::{diagnostic::Diagnostic, fixture::TestCaseFixtures};

#[derive(Debug, Clone)]
pub struct TestCase {
    file: SystemPathBuf,
    cwd: SystemPathBuf,
    function_definition: StmtFunctionDef,
}

impl TestCase {
    #[must_use]
    pub fn new(
        cwd: &SystemPathBuf,
        file: SystemPathBuf,
        function_definition: StmtFunctionDef,
    ) -> Self {
        Self {
            file,
            cwd: cwd.clone(),
            function_definition,
        }
    }

    #[must_use]
    pub const fn file(&self) -> &SystemPathBuf {
        &self.file
    }

    #[must_use]
    pub const fn cwd(&self) -> &SystemPathBuf {
        &self.cwd
    }

    #[must_use]
    pub const fn function_definition(&self) -> &StmtFunctionDef {
        &self.function_definition
    }

    #[must_use]
    pub fn get_required_fixtures(&self) -> Vec<String> {
        let mut required_fixtures = Vec::new();
        for parameter in self
            .function_definition
            .parameters
            .iter_non_variadic_params()
        {
            required_fixtures.push(parameter.parameter.name.as_str().to_string());
        }
        required_fixtures
    }

    #[must_use]
    pub fn run_test(
        &self,
        py: &Python,
        module: &Bound<'_, PyModule>,
        fixtures: &TestCaseFixtures<'_>,
    ) -> Option<Vec<Diagnostic>> {
        let result: PyResult<Bound<'_, PyAny>> = {
            let name: &str = &self.function_definition().name;
            let function = match module.getattr(name) {
                Ok(function) => function,
                Err(err) => {
                    return Some(vec![Diagnostic::from_py_err(py, &err)]);
                }
            };
            let required_fixture_names = self.get_required_fixtures();
            if required_fixture_names.is_empty() {
                function.call0()
            } else {
                let mut diagnostics = Vec::new();
                let required_fixtures = required_fixture_names
                    .iter()
                    .filter_map(|fixture| {
                        fixtures.get_fixture(fixture).map_or_else(
                            || {
                                diagnostics.push(Diagnostic::fixture_not_found(fixture));
                                None
                            },
                            Some,
                        )
                    })
                    .collect::<Vec<_>>();

                if !diagnostics.is_empty() {
                    return Some(diagnostics);
                }

                let args = PyTuple::new(*py, required_fixtures);
                match args {
                    Ok(args) => function.call(args, None),
                    Err(err) => Err(err),
                }
            }
        };
        match result {
            Ok(_) => None,
            Err(err) => Some(vec![Diagnostic::from_py_fail(py, &err)]),
        }
    }
}

impl Display for TestCase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}::{}",
            module_name(&self.cwd, &self.file),
            self.function_definition.name
        )
    }
}

impl Hash for TestCase {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.file.hash(state);
        self.function_definition.name.hash(state);
    }
}

impl PartialEq for TestCase {
    fn eq(&self, other: &Self) -> bool {
        self.file == other.file && self.function_definition.name == other.function_definition.name
    }
}

impl Eq for TestCase {}
