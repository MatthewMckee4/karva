use std::{
    cmp::{Eq, PartialEq},
    fmt::{self, Display},
    hash::{Hash, Hasher},
};

use karva_project::{path::SystemPathBuf, utils::module_name};
use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::diagnostic::Diagnostic;

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
    pub fn run_test(&self, py: &Python, module: &Bound<'_, PyModule>) -> Option<Diagnostic> {
        let result = {
            let name: &str = &self.function_definition().name;
            let function = match module.getattr(name) {
                Ok(function) => function,
                Err(err) => {
                    return Some(Diagnostic::from_py_err(py, &err));
                }
            };
            function.call0()
        };
        match result {
            Ok(_) => None,
            Err(err) => Some(Diagnostic::from_py_fail(py, &err)),
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
