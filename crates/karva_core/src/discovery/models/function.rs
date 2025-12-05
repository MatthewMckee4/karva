use std::sync::Arc;

use pyo3::prelude::*;
use ruff_python_ast::StmtFunctionDef;

use crate::{
    QualifiedFunctionName,
    discovery::DiscoveredModule,
    extensions::{fixtures::RequiresFixtures, tags::Tags},
};

/// Represents a single test function discovered from Python source code.
#[derive(Debug)]
pub struct TestFunction {
    /// The name of the test function.
    pub(crate) name: QualifiedFunctionName,

    /// The ast function statement.
    pub(crate) stmt_function_def: Arc<StmtFunctionDef>,

    /// The Python function object.
    pub(crate) py_function: Py<PyAny>,

    /// The tags associated with the test function.
    pub(crate) tags: Tags,
}

impl TestFunction {
    pub(crate) fn new(
        py: Python<'_>,
        module: &DiscoveredModule,
        stmt_function_def: Arc<StmtFunctionDef>,
        py_function: Py<PyAny>,
    ) -> Self {
        let name = QualifiedFunctionName::new(
            stmt_function_def.name.to_string(),
            module.module_path().clone(),
        );

        let tags = Tags::from_py_any(py, &py_function, Some(&stmt_function_def));

        Self {
            name,
            stmt_function_def,
            py_function,
            tags,
        }
    }
}

impl RequiresFixtures for TestFunction {
    fn required_fixtures(&self, py: Python<'_>) -> Vec<String> {
        let mut required_fixtures = self.stmt_function_def.required_fixtures(py);

        required_fixtures.extend(self.tags.required_fixtures_names());

        required_fixtures
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use karva_project::ProjectDatabase;
    use karva_test::TestContext;

    use crate::{
        Context, DummyReporter,
        discovery::{DiscoveredPackage, StandardDiscoverer},
        extensions::fixtures::RequiresFixtures,
        utils::attach,
    };

    #[allow(clippy::needless_pass_by_value)]
    fn context_to_discovered_package(
        context: &TestContext,
        paths: Vec<Utf8PathBuf>,
    ) -> DiscoveredPackage {
        let db = ProjectDatabase::test_db(context.cwd(), &paths);
        let binding = DummyReporter;
        let context = Context::new(&db, &binding);
        let discoverer = StandardDiscoverer::new(&context);
        discoverer.discover()
    }

    #[test]
    fn test_case_construction_and_getters() {
        let env = TestContext::with_files([("<test>/test.py", "def test_function(): pass")]);

        let session = context_to_discovered_package(&env, vec![env.cwd()]);

        let test_case = session.test_functions()[0];

        assert!(test_case.name.to_string().ends_with("test::test_function"));
    }

    #[test]
    fn test_case_with_fixtures() {
        attach(|py| {
            let env = TestContext::with_files([(
                "<test>/test.py",
                "def test_with_fixtures(fixture1, fixture2): pass",
            )]);

            let session = context_to_discovered_package(&env, vec![env.cwd()]);

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

        let session = context_to_discovered_package(&env, vec![env.cwd()]);

        let tests_package = session.get_package(mapped_dir).unwrap();

        let test_module = tests_package
            .get_module(&mapped_dir.join("test.py"))
            .unwrap();

        let test_case = session.test_functions()[0];

        assert_eq!(
            test_case.name.to_string(),
            format!("{}::test_display", test_module.name())
        );
    }
}
