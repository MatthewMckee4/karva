use std::{
    collections::HashMap,
    fmt::{self, Display},
};

use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::{
    collection::TestCase,
    diagnostic::{
        Diagnostic, DiagnosticErrorType, DiagnosticSeverity, SubDiagnostic,
        TestCaseCollectionDiagnosticType, TestCaseDiagnosticType,
    },
    discovery::DiscoveredModule,
    extensions::{
        fixtures::{FixtureManager, HasFunctionDefinition, RequiresFixtures},
        tags::Tags,
    },
    name::FunctionName,
    utils::Upcast,
};

/// Represents a single test function.
pub(crate) struct TestFunction {
    function_definition: StmtFunctionDef,
    py_function: Py<PyAny>,
    name: FunctionName,
}

impl HasFunctionDefinition for TestFunction {
    fn get_required_fixture_names(&self, py: Python<'_>) -> Vec<String> {
        let mut required_fixtures = self.function_definition.get_required_fixture_names(py);

        if let Some(tags) =
            Tags::from_py_any(py, &self.py_function, Some(&self.function_definition))
        {
            required_fixtures.extend(tags.use_fixtures_names());
        }

        required_fixtures
    }
}

impl TestFunction {
    #[must_use]
    pub(crate) fn new(
        module_name: String,
        function_definition: StmtFunctionDef,
        py_function: Py<PyAny>,
    ) -> Self {
        let name = FunctionName::new(function_definition.name.to_string(), module_name);

        Self {
            function_definition,
            py_function,
            name,
        }
    }

    #[must_use]
    pub(crate) const fn name(&self) -> &FunctionName {
        &self.name
    }

    #[must_use]
    pub(crate) fn function_name(&self) -> &str {
        &self.function_definition.name
    }

    #[must_use]
    pub(crate) const fn definition(&self) -> &StmtFunctionDef {
        &self.function_definition
    }

    pub(crate) fn display_with_line(&self, module: &DiscoveredModule) -> String {
        let line_index = module.line_index();
        let source_text = module.source_text();
        let start = self.function_definition.range.start();
        let line_number = line_index.line_column(start, &source_text);
        format!("{}:{}", module.path().display(), line_number.line)
    }

    pub(crate) fn collect<'a>(
        &'a self,
        py: Python<'_>,
        module: &'a DiscoveredModule,
        py_module: &Py<PyModule>,
        fixture_manager_func: impl Fn(
            Python<'_>,
            &dyn Fn(&FixtureManager<'_>) -> (TestCase<'a>, Option<Diagnostic>),
        ) -> (TestCase<'a>, Option<Diagnostic>)
        + Sync,
    ) -> Vec<(TestCase<'a>, Option<Diagnostic>)> {
        tracing::info!(
            "Collecting test cases for function: {}",
            self.function_definition.name
        );

        let Ok(py_function) = py_module.getattr(py, self.function_definition.name.to_string())
        else {
            return Vec::new();
        };

        let mut required_fixture_names = self.get_required_fixture_names(py);

        let tags = Tags::from_py_any(py, &py_function, Some(&self.function_definition));

        if let Some(tags) = &tags {
            let use_fixtures_names = tags.use_fixtures_names();

            required_fixture_names.extend(use_fixtures_names);
        }

        if required_fixture_names.is_empty() {
            return vec![(
                TestCase::new(self, HashMap::new(), py_function, module),
                None,
            )];
        }

        let mut parametrize_args = Vec::new();

        if let Some(tags) = &tags {
            parametrize_args.extend(tags.parametrize_args());
        }

        // Ensure that we collect at least one test case (no parametrization)
        if parametrize_args.is_empty() {
            parametrize_args.push(HashMap::new());
        }

        let mut test_cases = Vec::with_capacity(parametrize_args.len());

        for params in &parametrize_args {
            let f = |fixture_manager: &FixtureManager| {
                let num_required_fixtures = required_fixture_names.len();
                let mut fixture_diagnostics = Vec::with_capacity(num_required_fixtures);
                let mut required_fixtures = HashMap::with_capacity(num_required_fixtures);

                for fixture_name in &required_fixture_names {
                    if let Some(fixture) = params.get(fixture_name) {
                        required_fixtures.insert(fixture_name.clone(), fixture.clone());
                    } else if let Some(fixture) =
                        fixture_manager.get_fixture_with_name(fixture_name)
                    {
                        required_fixtures.insert(fixture_name.clone(), fixture);
                    } else {
                        fixture_diagnostics.push(SubDiagnostic::fixture_not_found(fixture_name));
                    }
                }

                let diagnostic = if fixture_diagnostics.is_empty() {
                    None
                } else {
                    let mut diagnostic = Diagnostic::new(
                        Some(format!("Fixture(s) not found for {}", self.name())),
                        Some(self.display_with_line(module)),
                        None,
                        DiagnosticSeverity::Error(DiagnosticErrorType::TestCase(
                            self.name().to_string(),
                            TestCaseDiagnosticType::Collection(
                                TestCaseCollectionDiagnosticType::FixtureNotFound,
                            ),
                        )),
                    );
                    diagnostic.add_sub_diagnostics(fixture_diagnostics);
                    Some(diagnostic)
                };

                (
                    TestCase::new(self, required_fixtures, py_function.clone(), module),
                    diagnostic,
                )
            };
            test_cases.push(fixture_manager_func(py, &f));
        }

        test_cases
    }

    pub(crate) const fn display(&self, module_name: String) -> TestFunctionDisplay<'_> {
        TestFunctionDisplay {
            test_function: self,
            module_name,
        }
    }
}

pub(crate) struct TestFunctionDisplay<'proj> {
    test_function: &'proj TestFunction,
    module_name: String,
}

impl Display for TestFunctionDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.module_name, self.test_function.name())
    }
}

impl<'proj> Upcast<Vec<&'proj dyn RequiresFixtures>> for Vec<&'proj TestFunction> {
    fn upcast(self) -> Vec<&'proj dyn RequiresFixtures> {
        let mut result = Vec::with_capacity(self.len());
        for tc in self {
            result.push(tc as &dyn RequiresFixtures);
        }
        result
    }
}

impl<'proj> Upcast<Vec<&'proj dyn HasFunctionDefinition>> for Vec<&'proj TestFunction> {
    fn upcast(self) -> Vec<&'proj dyn HasFunctionDefinition> {
        let mut result = Vec::with_capacity(self.len());
        for tc in self {
            result.push(tc as &dyn HasFunctionDefinition);
        }
        result
    }
}

impl std::fmt::Debug for TestFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {

    use karva_project::{project::Project, testing::TestEnv};
    use pyo3::prelude::*;

    use crate::{
        discovery::StandardDiscoverer,
        extensions::fixtures::{HasFunctionDefinition, RequiresFixtures},
    };

    #[test]
    fn test_case_construction_and_getters() {
        let env = TestEnv::with_files([("<test>/test.py", "def test_function(): pass")]);
        let path = env.create_file("test.py", "def test_function(): pass");

        let project = Project::new(env.cwd(), vec![path]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        let test_case = session.test_functions()[0];

        assert!(
            test_case
                .name()
                .to_string()
                .ends_with("test::test_function")
        );
    }

    #[test]
    fn test_case_with_fixtures() {
        Python::with_gil(|py| {
            let env = TestEnv::with_files([(
                "<test>/test.py",
                "def test_with_fixtures(fixture1, fixture2): pass",
            )]);

            let project = Project::new(env.cwd(), vec![env.cwd()]);
            let discoverer = StandardDiscoverer::new(&project);
            let (session, _) = Python::with_gil(|py| discoverer.discover(py));

            let test_case = session.test_functions()[0];

            let required_fixtures = test_case.get_required_fixture_names(py);
            assert_eq!(required_fixtures.len(), 2);
            assert!(required_fixtures.contains(&"fixture1".to_string()));
            assert!(required_fixtures.contains(&"fixture2".to_string()));

            assert!(test_case.uses_fixture(py, "fixture1"));
            assert!(test_case.uses_fixture(py, "fixture2"));
            assert!(!test_case.uses_fixture(py, "nonexistent"));
        });
    }

    #[test]
    fn test_case_display() {
        let env = TestEnv::with_files([("<test>/test.py", "def test_display(): pass")]);

        let mapped_dir = env.mapped_path("<test>").unwrap();

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::with_gil(|py| discoverer.discover(py));

        let tests_package = session.get_package(mapped_dir).unwrap();

        let test_module = tests_package
            .get_module(&mapped_dir.join("test.py"))
            .unwrap();

        let test_case = session.test_functions()[0];

        assert_eq!(
            test_case
                .display(test_module.name().to_string())
                .to_string(),
            format!("{}::test_display", test_module.name())
        );
    }
}
