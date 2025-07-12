use std::{
    collections::HashMap,
    fmt::{self, Display},
};

use karva_project::{path::SystemPathBuf, project::Project, utils::module_name};
use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::{
    collection::TestCase,
    diagnostic::Diagnostic,
    extensions::{
        fixtures::{FixtureManager, HasFunctionDefinition, RequiresFixtures},
        tags::Tags,
    },
    utils::Upcast,
};

/// Represents a single test function.
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

    pub fn collect<'a>(
        &'a self,
        py: Python<'_>,
        py_module: &Bound<'_, PyModule>,
        fixture_manager_func: &mut impl FnMut(
            &mut dyn FnMut(&mut FixtureManager) -> Result<TestCase<'a>, Diagnostic>,
        ) -> Result<TestCase<'a>, Diagnostic>,
    ) -> Vec<Result<TestCase<'a>, Diagnostic>> {
        tracing::info!("Collecting test cases for function: {}", self.name());
        let mut test_cases: Vec<Result<TestCase<'a>, Diagnostic>> = Vec::new();

        let name = self.function_definition().name.to_string();

        let Ok(py_function) = py_module.getattr(name) else {
            return test_cases;
        };
        let py_function = py_function.as_unbound();

        let module_name = self.module_name();

        let required_fixture_names = self.get_required_fixture_names();
        if required_fixture_names.is_empty() {
            test_cases.push(Ok(TestCase::new(
                self,
                vec![],
                py_function.clone(),
                module_name,
            )));
        } else {
            // The function requires fixtures or parameters, so we need to try to extract them from the test case.
            let tags = Tags::from_py_any(py, py_function);
            let mut param_args = tags.parametrize_args();

            // Ensure that there is at least one set of parameters.
            if param_args.is_empty() {
                param_args.push(HashMap::new());
            }

            for params in param_args {
                let mut f = |fixture_manager: &mut FixtureManager| {
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
                                Some(self.path.display().to_string()),
                            ));
                            None
                        })
                        .collect::<Vec<_>>();

                    // There are some not found fixtures.
                    if fixture_diagnostics.is_empty() {
                        Ok(TestCase::new(
                            self,
                            required_fixtures,
                            py_function.clone(),
                            module_name.clone(),
                        ))
                    } else {
                        Err(Diagnostic::from_test_diagnostics(fixture_diagnostics))
                    }
                };

                test_cases.push(fixture_manager_func(&mut f));
            }
        }

        test_cases
    }

    pub const fn display(&self, module_name: String) -> TestFunctionDisplay<'_> {
        TestFunctionDisplay {
            test_function: self,
            module_name,
        }
    }
}

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
        write!(f, "{}::{}", self.path.display(), self.name())
    }
}

#[cfg(test)]
mod tests {

    use karva_project::{project::Project, tests::TestEnv};
    use pyo3::prelude::*;

    use crate::{
        discovery::Discoverer,
        extensions::fixtures::{HasFunctionDefinition, RequiresFixtures},
    };

    #[test]
    fn test_case_construction_and_getters() {
        let env = TestEnv::new();
        let path = env.create_file("test.py", "def test_function(): pass");

        let project = Project::new(env.cwd(), vec![path.clone()]);
        let discoverer = Discoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        let test_case = session.test_functions()[0].clone();

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
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        let test_case = session.test_functions()[0].clone();

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
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        let test_case = session.test_functions()[0].clone();

        assert_eq!(
            test_case
                .display(session.modules().values().next().unwrap().name())
                .to_string(),
            "test::test_display"
        );
    }
}
