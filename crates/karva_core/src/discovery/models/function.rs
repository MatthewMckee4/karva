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
        fixtures::{FixtureManager, UsesFixtures},
        tags::Tags,
    },
    name::{ModulePath, QualifiedFunctionName},
    utils::Upcast,
};

/// Represents a single test function discovered from Python source code.
///
/// This structure bridges the gap between Rust's AST representation and Python's
/// runtime objects, maintaining both the parsed function definition and the actual
/// Python function object for execution.
#[derive(Debug)]
pub(crate) struct TestFunction {
    /// The parsed AST representation of the function from ruff
    function_definition: StmtFunctionDef,
    /// The actual Python function object that can be called
    py_function: Py<PyAny>,
    /// Qualified name including module path for unique identification
    name: QualifiedFunctionName,
}

impl UsesFixtures for TestFunction {
    fn dependant_fixtures(&self, py: Python<'_>) -> Vec<String> {
        let mut required_fixtures = self.function_definition.dependant_fixtures(py);

        let tags = Tags::from_py_any(py, &self.py_function, Some(&self.function_definition));

        required_fixtures.extend(tags.required_fixtures_names());

        required_fixtures
    }
}

impl TestFunction {
    pub(crate) fn new(
        module_path: ModulePath,
        function_definition: StmtFunctionDef,
        py_function: Py<PyAny>,
    ) -> Self {
        let name = QualifiedFunctionName::new(function_definition.name.to_string(), module_path);

        Self {
            function_definition,
            py_function,
            name,
        }
    }

    pub(crate) const fn name(&self) -> &QualifiedFunctionName {
        &self.name
    }

    pub(crate) fn function_name(&self) -> &str {
        &self.function_definition.name
    }

    pub(crate) const fn definition(&self) -> &StmtFunctionDef {
        &self.function_definition
    }

    pub(crate) const fn py_function(&self) -> &Py<PyAny> {
        &self.py_function
    }

    /// Creates a display string showing the function's file location and line number.
    ///
    /// This is particularly useful for error reporting and debugging, providing
    /// precise location information for test functions.
    pub(crate) fn display_with_line(&self, module: &DiscoveredModule) -> String {
        let line_index = module.line_index();
        let source_text = module.source_text();
        let start = self.function_definition.range.start();
        let line_number = line_index.line_column(start, source_text);
        format!("{}:{}", module.path().display(), line_number.line)
    }

    /// Collects test cases from this function, resolving fixtures and handling parametrization.
    ///
    /// This method is responsible for the complex process of converting a discovered test function
    /// into executable test cases. It handles fixture resolution, parametrization via tags,
    /// and creates the appropriate number of test cases based on parameter combinations.
    ///
    /// # Arguments
    /// * `py` - Python interpreter instance
    /// * `module` - The module containing this function
    /// * `py_module` - The Python module object
    /// * `fixture_manager_func` - Callback to create fixture managers for test execution
    ///
    /// # Returns
    /// A vector of test cases paired with optional diagnostics for any issues encountered
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

        let mut required_fixture_names = self.dependant_fixtures(py);

        let tags = Tags::from_py_any(py, &py_function, Some(&self.function_definition));

        required_fixture_names.extend(tags.required_fixtures_names());

        let mut parametrize_args = tags.parametrize_args();

        // Ensure at least one test case exists (no parametrization)
        if parametrize_args.is_empty() {
            parametrize_args.push(HashMap::new());
        }

        let mut test_cases = Vec::with_capacity(parametrize_args.len());

        for params in parametrize_args {
            let test_case_creator = |fixture_manager: &FixtureManager| {
                self.resolve_fixtures_for_test_case(
                    module,
                    &required_fixture_names,
                    &params,
                    fixture_manager,
                    &tags,
                )
            };
            test_cases.push(fixture_manager_func(py, &test_case_creator));
        }

        test_cases
    }

    fn resolve_fixtures_for_test_case<'a>(
        &'a self,
        module: &'a DiscoveredModule,
        required_fixture_names: &[String],
        params: &HashMap<String, Py<PyAny>>,
        fixture_manager: &FixtureManager,
        tags: &Tags,
    ) -> (TestCase<'a>, Option<Diagnostic>) {
        let num_required_fixtures = required_fixture_names.len();
        let mut fixture_diagnostics = Vec::with_capacity(num_required_fixtures);
        let mut resolved_fixtures = HashMap::with_capacity(num_required_fixtures);

        for fixture_name in required_fixture_names {
            if let Some(fixture_value) = params.get(fixture_name) {
                resolved_fixtures.insert(fixture_name.clone(), fixture_value.clone());
            } else if let Some(fixture_value) =
                fixture_manager.get_fixture_with_name(fixture_name, None)
            {
                resolved_fixtures.insert(fixture_name.clone(), fixture_value);
            } else {
                fixture_diagnostics.push(SubDiagnostic::fixture_not_found(fixture_name));
            }
        }

        let diagnostic = self.create_fixture_diagnostic(module, fixture_diagnostics);

        (
            TestCase::new(self, resolved_fixtures, module, tags.skip_tag()),
            diagnostic,
        )
    }

    fn create_fixture_diagnostic(
        &self,
        module: &DiscoveredModule,
        fixture_diagnostics: Vec<SubDiagnostic>,
    ) -> Option<Diagnostic> {
        if fixture_diagnostics.is_empty() {
            None
        } else {
            let mut diagnostic = Diagnostic::new(
                Some(format!("Fixture(s) not found for {}", self.name())),
                Some(self.display_with_line(module)),
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::TestCase {
                    test_name: self.name().to_string(),
                    diagnostic_type: TestCaseDiagnosticType::Collection(
                        TestCaseCollectionDiagnosticType::FixtureNotFound,
                    ),
                }),
            );
            diagnostic.add_sub_diagnostics(fixture_diagnostics);
            Some(diagnostic)
        }
    }

    pub(crate) const fn display(&self) -> TestFunctionDisplay<'_> {
        TestFunctionDisplay {
            test_function: self,
        }
    }
}

pub(crate) struct TestFunctionDisplay<'proj> {
    test_function: &'proj TestFunction,
}

impl Display for TestFunctionDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.test_function.name())
    }
}

impl<'proj> Upcast<Vec<&'proj dyn UsesFixtures>> for Vec<&'proj TestFunction> {
    fn upcast(self) -> Vec<&'proj dyn UsesFixtures> {
        let mut result = Vec::with_capacity(self.len());
        for tc in self {
            result.push(tc as &dyn UsesFixtures);
        }
        result
    }
}

#[cfg(test)]
mod tests {

    use karva_project::project::Project;
    use karva_test::TestContext;
    use pyo3::prelude::*;

    use crate::{discovery::StandardDiscoverer, extensions::fixtures::UsesFixtures};

    #[test]
    fn test_case_construction_and_getters() {
        let env = TestContext::with_files([("<test>/test.py", "def test_function(): pass")]);
        let path = env.create_file("test.py", "def test_function(): pass");

        let project = Project::new(env.cwd(), vec![path]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::attach(|py| discoverer.discover(py));

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
        Python::attach(|py| {
            let env = TestContext::with_files([(
                "<test>/test.py",
                "def test_with_fixtures(fixture1, fixture2): pass",
            )]);

            let project = Project::new(env.cwd(), vec![env.cwd()]);
            let discoverer = StandardDiscoverer::new(&project);
            let (session, _) = Python::attach(|py| discoverer.discover(py));

            let test_case = session.test_functions()[0];

            let required_fixtures = test_case.dependant_fixtures(py);
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
        let env = TestContext::with_files([("<test>/test.py", "def test_display(): pass")]);

        let mapped_dir = env.mapped_path("<test>").unwrap();

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let discoverer = StandardDiscoverer::new(&project);
        let (session, _) = Python::attach(|py| discoverer.discover(py));

        let tests_package = session.get_package(mapped_dir).unwrap();

        let test_module = tests_package
            .get_module(&mapped_dir.join("test.py"))
            .unwrap();

        let test_case = session.test_functions()[0];

        assert_eq!(
            test_case.display().to_string(),
            format!("{}::test_display", test_module.name())
        );
    }
}
