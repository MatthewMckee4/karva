use std::{
    cmp::{Eq, PartialEq},
    fmt::{self, Display},
    hash::{Hash, Hasher},
};

use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::{diagnostic::Diagnostic, discovery::module::Module};

#[derive(Debug, Clone)]
pub struct TestCase<'proj> {
    module: Module<'proj>,
    function_definition: StmtFunctionDef,
}

impl<'proj> TestCase<'proj> {
    #[must_use]
    pub const fn new(module: Module<'proj>, function_definition: StmtFunctionDef) -> Self {
        Self {
            module,
            function_definition,
        }
    }

    #[must_use]
    pub const fn module(&self) -> &Module<'proj> {
        &self.module
    }

    #[must_use]
    pub const fn function_definition(&self) -> &StmtFunctionDef {
        &self.function_definition
    }

    #[must_use]
    pub fn run_test(&self, py: Python, module: &Bound<'_, PyModule>) -> Option<Diagnostic> {
        let result = {
            let name: &str = &self.function_definition().name;
            let function = match module.getattr(name) {
                Ok(function) => function,
                Err(err) => return Some(Diagnostic::from_py_err(&err)),
            };
            function.call0()
        };
        match result {
            Ok(_) => None,
            Err(err) => Some(Diagnostic::from_fail(
                py,
                &self.module,
                &self.function_definition,
                &err,
            )),
        }
    }
}

impl Display for TestCase<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.module, self.function_definition.name)
    }
}

impl Hash for TestCase<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.module.hash(state);
        self.function_definition.name.hash(state);
    }
}

impl PartialEq for TestCase<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.module == other.module
            && self.function_definition.name == other.function_definition.name
    }
}

impl Eq for TestCase<'_> {}
