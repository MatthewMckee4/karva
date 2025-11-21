use std::collections::HashMap;

use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::{
    Context,
    diagnostic::{Diagnostic, FunctionKind},
    discovery::{DiscoveredModule, DiscoveredPackage},
    extensions::{
        fixtures::{FixtureManager, RequiresFixtures},
        tags::Tags,
    },
    name::{ModulePath, QualifiedFunctionName},
    normalize::models::NormalizedTestFunction,
    utils::function_definition_location,
};

/// Represents a single test function discovered from Python source code.
#[derive(Debug)]
pub(crate) struct TestFunction {
    function_definition: StmtFunctionDef,

    py_function: Py<PyAny>,

    name: QualifiedFunctionName,

    tags: Tags,
}

impl TestFunction {
    pub(crate) fn new(
        py: Python<'_>,
        module_path: ModulePath,
        function_definition: StmtFunctionDef,
        py_function: Py<PyAny>,
    ) -> Self {
        let name = QualifiedFunctionName::new(function_definition.name.to_string(), module_path);

        let tags = Tags::from_py_any(py, &py_function, Some(&function_definition));

        Self {
            function_definition,
            py_function,
            name,
            tags,
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

    pub(crate) const fn tags(&self) -> &Tags {
        &self.tags
    }

    /// Creates a display string showing the function's file location and line number.
    pub(crate) fn display_with_line(&self, module: &DiscoveredModule) -> String {
        function_definition_location(module, &self.function_definition)
    }

    pub(crate) fn normalize(
        &self,
        py: Python<'_>,
        context: &mut Context,
        module: &DiscoveredModule,
        parents: &[&DiscoveredPackage],
        fixture_manager: &mut FixtureManager,
    ) -> Vec<NormalizedTestFunction> {
        let mut required_fixture_names = self.function_definition.required_fixtures(py);

        required_fixture_names.extend(self.tags.required_fixtures_names());

        let mut parametrize_args = self.tags.parametrize_args();

        // Ensure at least one test case exists (no parametrization)
        if parametrize_args.is_empty() {
            parametrize_args.push(HashMap::new());
        }

        for params in parametrize_args {
            let mut missing_fixtures = Vec::new();

            // This is used to collect all fixture for each test case.
            // Currently, the only way we will generate more than one test case here is if
            // we have a parameterized fixture.
            let mut all_resolved_fixtures = Vec::new();

            all_resolved_fixtures.push(HashMap::new());

            for fixture_name in &required_fixture_names {
                if let Some(fixture_value) = params.get(fixture_name) {
                    // Add this parameterization to each test case.
                    for resolved_fixtures in &mut all_resolved_fixtures {
                        resolved_fixtures.insert(fixture_name.clone(), fixture_value.clone());
                    }
                } else if let Some(fixture_return) =
                    fixture_manager.get_fixture(py, context, parents, module, fixture_name, &[])
                {
                } else {
                    missing_fixtures.push(fixture_name.clone());
                }
            }

            let diagnostic = if missing_fixtures.is_empty() {
                None
            } else {
                let test_case_location = self.display_with_line(module);

                Some(Diagnostic::missing_fixtures(
                    missing_fixtures,
                    test_case_location,
                    self.name.to_string(),
                    FunctionKind::Test,
                ))
            };
        }

        // TODO: Implement proper normalization
        vec![]
    }
}

impl RequiresFixtures for TestFunction {
    fn required_fixtures(&self, py: Python<'_>) -> Vec<String> {
        let mut required_fixtures = self.function_definition.required_fixtures(py);

        required_fixtures.extend(self.tags.required_fixtures_names());

        required_fixtures
    }
}

#[cfg(test)]
mod tests {

    use karva_project::project::Project;
    use karva_test::TestContext;
    use pyo3::prelude::*;

    use crate::{discovery::StandardDiscoverer, extensions::fixtures::RequiresFixtures};

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

            let required_fixtures = test_case.required_fixtures(py);
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
            test_case.name().to_string(),
            format!("{}::test_display", test_module.name())
        );
    }
}
