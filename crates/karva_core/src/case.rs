use std::{
    cmp::{Eq, PartialEq},
    fmt::{self, Display},
    hash::{Hash, Hasher},
};

use karva_project::{path::SystemPathBuf, utils::module_name};
use pyo3::{prelude::*, types::PyTuple};
use ruff_python_ast::StmtFunctionDef;

use crate::{
    diagnostic::{Diagnostic, DiagnosticScope},
    fixture::TestCaseFixtures,
};

/// A test case represents a single test function.
#[derive(Debug, Clone)]
pub struct TestCase {
    path: SystemPathBuf,
    cwd: SystemPathBuf,
    function_definition: StmtFunctionDef,
}

impl TestCase {
    #[must_use]
    pub fn new(
        cwd: &SystemPathBuf,
        path: SystemPathBuf,
        function_definition: StmtFunctionDef,
    ) -> Self {
        Self {
            path,
            cwd: cwd.clone(),
            function_definition,
        }
    }

    #[must_use]
    pub const fn path(&self) -> &SystemPathBuf {
        &self.path
    }

    #[must_use]
    pub const fn cwd(&self) -> &SystemPathBuf {
        &self.cwd
    }

    #[must_use]
    pub fn name(&self) -> String {
        self.function_definition.name.to_string()
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
        py: Python<'_>,
        module: &Bound<'_, PyModule>,
        fixtures: &TestCaseFixtures<'_>,
    ) -> Option<Diagnostic> {
        let result: PyResult<Bound<'_, PyAny>> = {
            let name: &str = &self.function_definition().name;
            let function = match module.getattr(name) {
                Ok(function) => function,
                Err(err) => {
                    return Some(Diagnostic::from_py_err(
                        py,
                        &err,
                        DiagnosticScope::Test,
                        &self.name(),
                    ));
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
                                diagnostics.push(Diagnostic::fixture_not_found(fixture, self));
                                None
                            },
                            Some,
                        )
                    })
                    .collect::<Vec<_>>();

                if !diagnostics.is_empty() {
                    return Some(Diagnostic::from_test_diagnostics(diagnostics));
                }

                let args = PyTuple::new(py, required_fixtures);
                match args {
                    Ok(args) => function.call(args, None),
                    Err(err) => Err(err),
                }
            }
        };
        match result {
            Ok(_) => None,
            Err(err) => Some(Diagnostic::from_test_fail(py, &err, self)),
        }
    }

    #[must_use]
    pub fn uses_fixture(&self, fixture_name: &str) -> bool {
        self.get_required_fixtures()
            .contains(&fixture_name.to_string())
    }
}

impl Display for TestCase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}::{}",
            module_name(&self.cwd, &self.path),
            self.function_definition.name
        )
    }
}

impl Hash for TestCase {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.function_definition.name.hash(state);
    }
}

impl PartialEq for TestCase {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.function_definition.name == other.function_definition.name
    }
}

impl Eq for TestCase {}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use karva_project::{project::Project, tests::TestEnv, utils::module_name};
    use pyo3::{prelude::*, types::PyModule};

    use crate::{discovery::Discoverer, fixture::TestCaseFixtures, utils::add_to_sys_path};

    #[test]
    fn test_case_construction_and_getters() {
        let env = TestEnv::new();
        let path = env.create_file("test.py", "def test_function(): pass");

        let project = Project::new(env.cwd(), vec![path.clone()]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        let test_case = session.test_cases()[0].clone();

        assert_eq!(test_case.path(), &path);
        assert_eq!(test_case.cwd(), &env.cwd());
        assert_eq!(test_case.name(), "test_function");
    }

    #[test]
    fn test_case_with_fixtures() {
        let env = TestEnv::new();
        let path = env.create_file(
            "test.py",
            "def test_with_fixtures(fixture1, fixture2): pass",
        );

        let project = Project::new(env.cwd(), vec![path]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        let test_case = session.test_cases()[0].clone();

        let required_fixtures = test_case.get_required_fixtures();
        assert_eq!(required_fixtures.len(), 2);
        assert!(required_fixtures.contains(&"fixture1".to_string()));
        assert!(required_fixtures.contains(&"fixture2".to_string()));

        assert!(test_case.uses_fixture("fixture1"));
        assert!(test_case.uses_fixture("fixture2"));
        assert!(!test_case.uses_fixture("nonexistent"));
    }

    #[test]
    fn test_case_display() {
        let env = TestEnv::new();
        let path = env.create_file("test.py", "def test_display(): pass");

        let project = Project::new(env.cwd(), vec![path]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        let test_case = session.test_cases()[0].clone();

        assert_eq!(test_case.to_string(), "test::test_display");
    }

    #[test]
    fn test_case_equality() {
        let env = TestEnv::new();
        let path1 = env.create_file("test1.py", "def test_same(): pass");
        let path2 = env.create_file("test2.py", "def test_different(): pass");

        let project = Project::new(env.cwd(), vec![path1, path2]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        let test_case1 = session.test_cases()[0].clone();
        let test_case2 = session.test_cases()[1].clone();

        assert_eq!(test_case1, test_case1);
        assert_ne!(test_case1, test_case2);
    }

    #[test]
    fn test_case_hash() {
        use std::collections::HashSet;

        let env = TestEnv::new();
        let path1 = env.create_file("test1.py", "def test_same(): pass");
        let path2 = env.create_file("test2.py", "def test_different(): pass");

        let project = Project::new(env.cwd(), vec![path1, path2]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        let test_case1 = session.test_cases()[0].clone();
        let test_case2 = session.test_cases()[1].clone();

        let mut set = HashSet::new();
        set.insert(test_case1.clone());
        assert!(!set.contains(&test_case2));
        assert!(set.contains(&test_case1));
    }

    #[test]
    fn test_run_test_without_fixtures() {
        let env = TestEnv::new();
        let path = env.create_file("tests/test.py", "def test_simple(): pass");

        let project = Project::new(env.cwd(), vec![path.clone()]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = discoverer.discover();

        let test_case = session.test_cases()[0].clone();
        Python::with_gil(|py| {
            add_to_sys_path(&py, &env.cwd()).unwrap();
            let module = PyModule::import(py, module_name(&env.cwd(), &path)).unwrap();
            let temp_fixtures = HashMap::new();
            let fixtures = TestCaseFixtures::new(&temp_fixtures);
            let result = test_case.run_test(py, &module, &fixtures);
            assert!(result.is_none());
        });
    }
}
