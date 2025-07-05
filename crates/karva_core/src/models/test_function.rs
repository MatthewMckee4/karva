use std::{
    cmp::{Eq, PartialEq},
    collections::HashMap,
    fmt::{self, Display},
    hash::{Hash, Hasher},
};

use karva_project::{path::SystemPathBuf, project::Project, utils::module_name};
use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::{
    diagnostic::{Diagnostic, DiagnosticScope, Severity},
    fixture::{FixtureManager, HasFunctionDefinition, RequiresFixtures},
    models::TestCase,
    tag::Tags,
    utils::Upcast,
};

/// A test case represents a single test function.
#[derive(Clone)]
pub struct TestFunction<'proj> {
    project: &'proj Project,
    path: SystemPathBuf,
    function_definition: StmtFunctionDef,
}

impl HasFunctionDefinition for TestFunction<'_> {
    fn function_definition(&self) -> &StmtFunctionDef {
        &self.function_definition
    }
}

impl<'proj> TestFunction<'proj> {
    #[must_use]
    pub const fn new(
        project: &'proj Project,
        path: SystemPathBuf,
        function_definition: StmtFunctionDef,
    ) -> Self {
        Self {
            project,
            path,
            function_definition,
        }
    }

    #[must_use]
    pub const fn path(&self) -> &SystemPathBuf {
        &self.path
    }

    #[must_use]
    pub fn name(&self) -> String {
        self.function_definition.name.to_string()
    }

    #[must_use]
    pub fn module_name(&self) -> String {
        module_name(self.project.cwd(), &self.path)
    }

    pub fn collect<'a: 'proj>(
        &'a self,
        py: Python<'_>,
        py_module: &Bound<'_, PyModule>,
        fixture_manager_func: &mut impl FnMut(
            &dyn Fn(&FixtureManager) -> Result<TestCase<'a>, Diagnostic>,
        ) -> Result<TestCase<'a>, Diagnostic>,
    ) -> Result<Vec<Result<TestCase<'a>, Diagnostic>>, Diagnostic> {
        let mut test_cases: Vec<Result<TestCase<'a>, Diagnostic>> = Vec::new();

        let name = self.function_definition().name.to_string();

        let py_function = match py_module.getattr(name) {
            Ok(function) => function,
            Err(err) => {
                return Err(Diagnostic::from_py_err(
                    py,
                    &err,
                    DiagnosticScope::Test,
                    Some(self.name()),
                    Severity::Warning("Unknown".to_string()),
                ));
            }
        };
        let function = py_function.as_unbound();

        let required_fixture_names = self.get_required_fixture_names();
        if required_fixture_names.is_empty() {
            test_cases.push(Ok(TestCase::new(
                self,
                vec![],
                py_module.as_unbound().clone(),
            )));
        } else {
            // The function requires fixtures or parameters, so we need to try to extract them from the test case.
            let tags = Tags::from_py_any(py, function);
            let mut param_args = tags.parametrize_args();

            // Ensure that there is at least one set of parameters.
            if param_args.is_empty() {
                param_args.push(HashMap::new());
            }

            for params in param_args {
                let mut f = |fixture_manager: &FixtureManager| {
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
                                Some(self.path.to_string()),
                            ));
                            None
                        })
                        .collect::<Vec<_>>();

                    // There are some not found fixtures.

                    if fixture_diagnostics.is_empty() {
                        Ok(TestCase::new(
                            self,
                            required_fixtures,
                            py_module.as_unbound().clone(),
                        ))
                    } else {
                        Err(Diagnostic::from_test_diagnostics(fixture_diagnostics))
                    }
                };

                test_cases.push(fixture_manager_func(&mut f));
            }
        }
        Ok(test_cases)
    }

    pub const fn display(&self, module_name: String) -> TestFunctionDisplay<'_> {
        TestFunctionDisplay {
            test_function: self,
            module_name,
        }
    }
}

impl Hash for TestFunction<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.function_definition.name.hash(state);
    }
}

impl PartialEq for TestFunction<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.function_definition.name == other.function_definition.name
    }
}

impl Eq for TestFunction<'_> {}

pub struct TestFunctionDisplay<'proj> {
    test_function: &'proj TestFunction<'proj>,
    module_name: String,
}

impl Display for TestFunctionDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.module_name, self.test_function.name())
    }
}

impl<'proj> Upcast<Vec<&'proj dyn RequiresFixtures>> for Vec<&'proj TestFunction<'proj>> {
    fn upcast(self) -> Vec<&'proj dyn RequiresFixtures> {
        self.into_iter()
            .map(|tc| tc as &dyn RequiresFixtures)
            .collect()
    }
}

impl<'proj> Upcast<Vec<&'proj dyn HasFunctionDefinition>> for Vec<&'proj TestFunction<'proj>> {
    fn upcast(self) -> Vec<&'proj dyn HasFunctionDefinition> {
        self.into_iter()
            .map(|tc| tc as &dyn HasFunctionDefinition)
            .collect()
    }
}

impl std::fmt::Debug for TestFunction<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TestCase(path: {}, name: {})", self.path, self.name())
    }
}

#[cfg(test)]
mod tests {

    use karva_project::{project::Project, tests::TestEnv};

    use crate::{
        discovery::Discoverer,
        fixture::{HasFunctionDefinition, RequiresFixtures},
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

        assert_eq!(
            test_case
                .display(session.modules().values().next().unwrap().name())
                .to_string(),
            "test::test_display"
        );
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
}
