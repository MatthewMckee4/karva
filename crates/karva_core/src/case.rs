use std::{
    cmp::{Eq, PartialEq},
    collections::HashMap,
    fmt::{self, Display},
    hash::{Hash, Hasher},
};

use karva_project::{path::SystemPathBuf, utils::module_name};
use pyo3::{IntoPyObjectExt, prelude::*, types::PyTuple};
use ruff_python_ast::StmtFunctionDef;

use crate::{
    diagnostic::{Diagnostic, DiagnosticScope, SubDiagnosticType},
    fixture::{FixtureManager, HasFunctionDefinition, RequiresFixtures},
    tag::{Tag, Tags},
    utils::Upcast,
};

/// A test case represents a single test function.
#[derive(Clone)]
pub struct TestFunction {
    path: SystemPathBuf,
    cwd: SystemPathBuf,
    function_definition: StmtFunctionDef,
}

impl HasFunctionDefinition for TestFunction {
    fn function_definition(&self) -> &StmtFunctionDef {
        &self.function_definition
    }
}

impl TestFunction {
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
    pub fn test(
        &self,
        py: Python<'_>,
        module: &Bound<'_, PyModule>,
        fixture_manager: &FixtureManager,
    ) -> TestFunctionRunResult {
        let mut run_result = TestFunctionRunResult::default();

        let name: &str = &self.function_definition().name;
        let function = match module.getattr(name) {
            Ok(function) => function,
            Err(err) => {
                run_result.diagnostics.push(Diagnostic::from_py_err(
                    py,
                    &err,
                    DiagnosticScope::Test,
                    &self.name(),
                ));
                return run_result;
            }
        };
        let function = function.as_unbound();

        let required_fixture_names = self.get_required_fixture_names();
        if required_fixture_names.is_empty() {
            match function.call0(py) {
                Ok(_) => {
                    run_result.result.add_passed();
                }
                Err(err) => {
                    let diagnostic = Diagnostic::from_test_fail(py, &err, self);
                    match diagnostic.diagnostic_type() {
                        SubDiagnosticType::Fail => run_result.result.add_failed(),
                        SubDiagnosticType::Error(_) => run_result.result.add_errored(),
                    }
                    run_result.diagnostics.push(diagnostic);
                }
            }
        } else {
            // The function requires fixtures or parameters, so we need to extract them from the test case.
            let mut param_args: Vec<HashMap<String, PyObject>> = Vec::new();
            let tags = Tags::from_py_any(py, function);
            for tag in tags {
                match tag {
                    Tag::Parametrize(parametrize_tag) => {
                        param_args.extend(parametrize_tag.each_arg_value());
                    }
                }
            }

            if param_args.is_empty() {
                param_args.push(HashMap::new());
            }

            let results = param_args
                .iter()
                .map(|params| {
                    let mut inner_run_result = TestFunctionRunResult::default();
                    let mut fixture_diagnostics = Vec::new();
                    let required_fixtures = required_fixture_names
                        .iter()
                        .filter_map(|fixture| {
                            params.get(fixture).map_or_else(
                                || {
                                    fixture_manager.get_fixture(fixture).map_or_else(
                                        || {
                                            fixture_diagnostics.push(
                                                Diagnostic::fixture_not_found(
                                                    fixture,
                                                    &self.path.to_string(),
                                                ),
                                            );
                                            None
                                        },
                                        |fixture| fixture.into_py_any(py).ok(),
                                    )
                                },
                                |result| Some(result.clone()),
                            )
                        })
                        .collect::<Vec<_>>();

                    if !fixture_diagnostics.is_empty() {
                        inner_run_result
                            .diagnostics
                            .push(Diagnostic::from_test_diagnostics(fixture_diagnostics));
                        inner_run_result.result.add_errored();
                        return inner_run_result;
                    }

                    let args = PyTuple::new(py, required_fixtures);

                    match args {
                        Ok(args) => {
                            if args.is_empty() {
                                tracing::info!("Running test: {}", self.to_string());
                            } else {
                                tracing::info!("Running test: {}[{:?}]", self.to_string(), args);
                            }
                            match function.call1(py, args) {
                                Ok(_) => {
                                    tracing::info!("Test passed");
                                    inner_run_result.result.add_passed();
                                }
                                Err(err) => {
                                    let diagnostic = Diagnostic::from_test_fail(py, &err, self);
                                    match diagnostic.diagnostic_type() {
                                        SubDiagnosticType::Fail => {
                                            inner_run_result.result.add_failed();
                                            tracing::info!("Test failed");
                                        }
                                        SubDiagnosticType::Error(_) => {
                                            inner_run_result.result.add_errored();
                                            tracing::info!("Test errored");
                                        }
                                    }
                                    inner_run_result.diagnostics.push(diagnostic);
                                }
                            }
                        }
                        Err(err) => {
                            inner_run_result.diagnostics.push(Diagnostic::unknown_error(
                                &err.to_string(),
                                &self.to_string(),
                            ));
                            inner_run_result.result.add_errored();
                        }
                    }
                    inner_run_result
                })
                .collect::<Vec<_>>();

            for result in results {
                run_result.update(&result);
            }
        }
        run_result
    }
}

impl Display for TestFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}::{}",
            module_name(&self.cwd, &self.path),
            self.function_definition.name
        )
    }
}

impl Hash for TestFunction {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.function_definition.name.hash(state);
    }
}

impl PartialEq for TestFunction {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.function_definition.name == other.function_definition.name
    }
}

impl Eq for TestFunction {}

impl<'a> Upcast<Vec<&'a dyn RequiresFixtures>> for Vec<&'a TestFunction> {
    fn upcast(self) -> Vec<&'a dyn RequiresFixtures> {
        self.into_iter()
            .map(|tc| tc as &dyn RequiresFixtures)
            .collect()
    }
}

impl<'a> Upcast<Vec<&'a dyn HasFunctionDefinition>> for Vec<&'a TestFunction> {
    fn upcast(self) -> Vec<&'a dyn HasFunctionDefinition> {
        self.into_iter()
            .map(|tc| tc as &dyn HasFunctionDefinition)
            .collect()
    }
}

impl std::fmt::Debug for TestFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TestCase(path: {}, name: {})", self.path, self.name())
    }
}

#[derive(Clone, Debug, Default)]
pub struct TestFunctionRunResult {
    pub diagnostics: Vec<Diagnostic>,
    pub result: TestFunctionRunStats,
}

impl TestFunctionRunResult {
    pub fn update(&mut self, other: &Self) {
        self.diagnostics.extend(other.diagnostics.clone());
        self.result.update(&other.result);
    }
}

#[derive(Clone, Debug, Default)]
pub struct TestFunctionRunStats {
    total: usize,
    passed: usize,
    failed: usize,
    errored: usize,
}

impl TestFunctionRunStats {
    #[must_use]
    pub const fn total(&self) -> usize {
        self.total
    }

    #[must_use]
    pub const fn passed(&self) -> usize {
        self.passed
    }

    #[must_use]
    pub const fn failed(&self) -> usize {
        self.failed
    }

    #[must_use]
    pub const fn errored(&self) -> usize {
        self.errored
    }

    pub const fn add_failed(&mut self) {
        self.failed += 1;
        self.total += 1;
    }

    pub const fn add_errored(&mut self) {
        self.errored += 1;
        self.total += 1;
    }

    pub const fn add_passed(&mut self) {
        self.passed += 1;
        self.total += 1;
    }

    pub const fn update(&mut self, other: &Self) {
        self.total += other.total;
        self.passed += other.passed;
        self.failed += other.failed;
        self.errored += other.errored;
    }
}

#[cfg(test)]
mod tests {

    use karva_project::{project::Project, tests::TestEnv, utils::module_name};
    use pyo3::{prelude::*, types::PyModule};

    use crate::{
        discovery::Discoverer,
        fixture::{FixtureManager, HasFunctionDefinition, RequiresFixtures},
        utils::add_to_sys_path,
    };

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

        let required_fixtures = test_case.get_required_fixture_names();
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
            let fixture_manager = FixtureManager::new();
            let result = test_case.test(py, &module, &fixture_manager);
            assert!(result.diagnostics.is_empty());
        });
    }
}
