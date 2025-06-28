use std::{
    cmp::{Eq, PartialEq},
    collections::HashMap,
    fmt::{self, Display},
    hash::{Hash, Hasher},
};

use karva_project::{path::SystemPathBuf, utils::module_name};
use pyo3::{prelude::*, types::PyTuple};
use ruff_python_ast::StmtFunctionDef;

use crate::{
    diagnostic::{Diagnostic, DiagnosticScope, SubDiagnosticType},
    fixture::{FixtureManager, HasFunctionDefinition, RequiresFixtures},
    runner::RunDiagnostics,
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
    ) -> RunDiagnostics {
        let mut run_result = RunDiagnostics::default();

        let name = self.function_definition().name.to_string();

        let function = match module.getattr(name) {
            Ok(function) => function,
            Err(err) => {
                run_result.add_diagnostic(Diagnostic::from_py_err(
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
                    run_result.stats_mut().add_passed();
                }
                Err(err) => {
                    run_result.add_diagnostic(Diagnostic::from_test_fail(py, &err, self));
                }
            }
        } else {
            // The function requires fixtures or parameters, so we need to try to extract them from the test case.
            let mut param_args: Vec<HashMap<String, PyObject>> = Vec::new();
            let tags = Tags::from_py_any(py, function);
            for tag in tags {
                match tag {
                    Tag::Parametrize(parametrize_tag) => {
                        param_args.extend(parametrize_tag.each_arg_value());
                    }
                }
            }

            // Ensure that there is at least one set of parameters.
            if param_args.is_empty() {
                param_args.push(HashMap::new());
            }

            for params in param_args {
                let mut inner_run_result = RunDiagnostics::default();
                let mut fixture_diagnostics = Vec::new();

                let required_fixtures = required_fixture_names
                    .iter()
                    .filter_map(|fixture| {
                        if let Some(fixture) = params.get(fixture) {
                            return Some(fixture.clone());
                        }

                        if let Some(fixture) = fixture_manager.get_fixture(fixture) {
                            return Some(fixture);
                        }

                        fixture_diagnostics.push(Diagnostic::fixture_not_found(
                            fixture,
                            &self.path.to_string(),
                        ));
                        None
                    })
                    .collect::<Vec<_>>();

                // There are some not found fixtures.
                if fixture_diagnostics.is_empty() {
                    let test_function_arguments = PyTuple::new(py, required_fixtures);

                    match test_function_arguments {
                        Ok(args) => {
                            let test_name = if args.is_empty() {
                                self.to_string()
                            } else {
                                format!("{self} [{args:?}]")
                            };
                            tracing::info!("Running test: {test_name}");
                            match function.call1(py, args) {
                                Ok(_) => {
                                    tracing::info!("Test {test_name} passed");
                                    inner_run_result.stats_mut().add_passed();
                                }
                                Err(err) => {
                                    let diagnostic = Diagnostic::from_test_fail(py, &err, self);
                                    match diagnostic.diagnostic_type() {
                                        SubDiagnosticType::Fail => {
                                            tracing::info!("Test {test_name} failed");
                                        }
                                        SubDiagnosticType::Error(_) => {
                                            tracing::info!("Test {test_name} errored");
                                        }
                                    }
                                    inner_run_result.add_diagnostic(diagnostic);
                                }
                            }
                        }
                        Err(err) => {
                            inner_run_result.add_diagnostic(Diagnostic::unknown_error(
                                &err.to_string(),
                                &self.to_string(),
                            ));
                        }
                    }
                } else {
                    inner_run_result
                        .add_diagnostic(Diagnostic::from_test_diagnostics(fixture_diagnostics));
                }
                run_result.update(&inner_run_result);
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

#[cfg(test)]
mod tests {

    use karva_project::{project::Project, tests::TestEnv, utils::module_name};
    use pyo3::{prelude::*, types::PyModule};

    use crate::{
        discovery::Discoverer,
        fixture::{FixtureManager, HasFunctionDefinition, RequiresFixtures},
        runner::DiagnosticStats,
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
            assert!(result.is_empty());
        });
    }

    #[test]
    fn test_parametrize() {
        let env = TestEnv::new();
        let test_dir = env.create_tests_dir();
        let path = env.create_file(
            test_dir.join("test_parametrize.py").as_ref(),
            r#"import karva
@karva.tags.parametrize(("a", "b"), [(1, 2), (1, 3)])
def test_parametrize(a, b):
    assert a < b"#,
        );

        let project = Project::new(env.cwd(), vec![test_dir]);
        let discoverer = Discoverer::new(&project);

        let (session, _) = discoverer.discover();

        let test_case = session.test_cases()[0].clone();
        Python::with_gil(|py| {
            add_to_sys_path(&py, &env.cwd()).unwrap();
            let module = PyModule::import(py, module_name(&env.cwd(), &path)).unwrap();
            let fixture_manager = FixtureManager::new();
            let result = test_case.test(py, &module, &fixture_manager);
            assert!(result.is_empty());

            let mut expected_stats = DiagnosticStats::default();
            expected_stats.add_passed();
            expected_stats.add_passed();
            assert_eq!(*result.stats(), expected_stats);
        });
    }

    #[test]
    fn test_parametrize_single_parameter() {
        let env = TestEnv::new();
        let test_dir = env.create_tests_dir();
        let path = env.create_file(
            test_dir.join("test_parametrize.py").as_ref(),
            r#"import karva
@karva.tags.parametrize("a", [1, 2, 3])
def test_parametrize(a):
    assert a > 0"#,
        );

        let project = Project::new(env.cwd(), vec![test_dir]);
        let discoverer = Discoverer::new(&project);

        let (session, _) = discoverer.discover();

        let test_case = session.test_cases()[0].clone();
        Python::with_gil(|py| {
            add_to_sys_path(&py, &env.cwd()).unwrap();
            let module = PyModule::import(py, module_name(&env.cwd(), &path)).unwrap();
            let fixture_manager = FixtureManager::new();
            let result = test_case.test(py, &module, &fixture_manager);
            assert!(result.is_empty());

            let mut expected_stats = DiagnosticStats::default();
            expected_stats.add_passed();
            expected_stats.add_passed();
            expected_stats.add_passed();
            assert_eq!(*result.stats(), expected_stats);
        });
    }
}
