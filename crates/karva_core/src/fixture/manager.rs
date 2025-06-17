use std::collections::HashMap;

use pyo3::{prelude::*, types::PyAny};

use crate::fixture::{Fixture, FixtureScope, HasFixtures, UsesFixture};

#[derive(Debug, Default)]
pub struct FixtureManager<'proj> {
    session_fixtures: HashMap<String, Bound<'proj, PyAny>>,
    module_fixtures: HashMap<String, Bound<'proj, PyAny>>,
    package_fixtures: HashMap<String, Bound<'proj, PyAny>>,
    function_fixtures: HashMap<String, Bound<'proj, PyAny>>,
}

impl<'proj> FixtureManager<'proj> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            session_fixtures: HashMap::new(),
            module_fixtures: HashMap::new(),
            package_fixtures: HashMap::new(),
            function_fixtures: HashMap::new(),
        }
    }

    #[must_use]
    pub fn get_fixture(&self, fixture_name: &str) -> Option<Bound<'proj, PyAny>> {
        self.all_fixtures().get(fixture_name).cloned()
    }

    #[must_use]
    pub fn all_fixtures(&self) -> HashMap<String, Bound<'proj, PyAny>> {
        let mut fixtures = HashMap::new();
        fixtures.extend(
            self.session_fixtures
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        fixtures.extend(
            self.module_fixtures
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        fixtures.extend(
            self.package_fixtures
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        fixtures.extend(
            self.function_fixtures
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        fixtures
    }

    // TODO: This is a bit of a mess.
    // This is sued to recursively resolve all of the dependencies of a fixture.
    fn ensure_fixture_dependencies(
        &self,
        py: Python<'proj>,
        parents: Vec<&'proj dyn HasFixtures<'proj>>,
        current: &'proj dyn HasFixtures<'proj>,
        scopes: &[FixtureScope],
        fixture: &'proj Fixture,
        called_fixtures: &mut HashMap<String, Bound<'proj, PyAny>>,
    ) {
        if let Some(fixture_return) = self.get_fixture(fixture.name()) {
            // We have already called this fixture. So we can just return.
            called_fixtures.insert(fixture.name().to_string(), fixture_return);
            return;
        } else if called_fixtures.contains_key(fixture.name()) {
            // We have already called this fixture. So we can just return.
            return;
        }

        println!("fixture: {:?}", fixture);

        // To ensure we can call the current fixture, we must first look at all of its dependencies,
        // and resolve them first.
        let current_dependencies = fixture.dependencies();

        println!("current_dependencies: {:?}", current_dependencies);

        // We need to get all of the fixtures in the current scope.
        let current_all_fixtures = current.all_fixtures(Vec::new());

        for dependency in &current_dependencies {
            let mut found = false;
            for fixture in &current_all_fixtures {
                if fixture.name() == dependency {
                    self.ensure_fixture_dependencies(
                        py,
                        parents.clone(),
                        current,
                        scopes,
                        fixture,
                        called_fixtures,
                    );
                    found = true;
                    break;
                }
            }

            // We did not find the dependency in the current scope.
            // So we must try the parent scopes.
            if !found {
                let mut parents_above_current_parent = parents.clone();
                let mut i = parents.len();
                while i > 0 {
                    i -= 1;
                    let parent = &parents[i];
                    parents_above_current_parent.truncate(i);

                    let parent_fixture = parent.get_fixture(dependency);

                    if let Some(parent_fixture) = parent_fixture {
                        self.ensure_fixture_dependencies(
                            py,
                            parents_above_current_parent.clone(),
                            *parent,
                            scopes,
                            parent_fixture,
                            called_fixtures,
                        );
                    } else {
                        tracing::error!(
                            "Failed to find fixture {} in parent {:?}",
                            dependency,
                            parent
                        );
                    }
                    if called_fixtures.contains_key(dependency) {
                        break;
                    }
                }
            }
        }

        let mut required_fixtures = Vec::new();

        for name in current_dependencies {
            if let Some(fixture) = called_fixtures.get(&name) {
                required_fixtures.push(fixture.clone());
            } else {
                tracing::error!("Failed to find fixture {}", name);
                return;
            }
        }

        // I think we can be sure that required_fixtures

        match fixture.call(py, required_fixtures) {
            Ok(fixture_return) => {
                called_fixtures.insert(fixture.name().to_string(), fixture_return);
            }
            Err(e) => {
                tracing::error!("Failed to call fixture {}: {}", fixture.name(), e);
            }
        }
    }

    // TODO: This is a bit of a mess.
    // This used to ensure that all of the given dependencies (fixtures) have been called.
    // This first starts with finding all dependencies of the given fixtures, and resolving and calling them first.
    //
    // We take the parents to ensure that if the dependent fixtures are not in the current scope,
    // we can still look for them in the parents.
    fn add_fixtures_impl(
        &self,
        py: Python<'proj>,
        parents: Vec<&'proj dyn HasFixtures<'proj>>,
        current: &'proj dyn HasFixtures<'proj>,
        scopes: &[FixtureScope],
        dependencies: Vec<&dyn UsesFixture>,
    ) -> HashMap<String, Bound<'proj, PyAny>> {
        let fixtures = current.fixtures(scopes, dependencies.clone());

        println!("fixtures: {:?}", fixtures);
        let mut called_fixtures: HashMap<String, Bound<'proj, PyAny>> = HashMap::new();

        if fixtures.is_empty() {
            return called_fixtures;
        }

        for fixture in &fixtures {
            self.ensure_fixture_dependencies(
                py,
                parents.clone(),
                current,
                scopes,
                fixture,
                &mut called_fixtures,
            );
        }

        called_fixtures
    }

    pub fn add_session_fixtures(
        &mut self,
        py: Python<'proj>,
        parents: Vec<&'proj dyn HasFixtures<'proj>>,
        current: &'proj dyn HasFixtures<'proj>,
        scope: &[FixtureScope],
        dependencies: Vec<&dyn UsesFixture>,
    ) {
        let called_fixtures = self.add_fixtures_impl(py, parents, current, scope, dependencies);
        self.session_fixtures.extend(called_fixtures);
    }

    pub fn reset_session_fixtures(&mut self) {
        self.session_fixtures.clear();
    }

    pub fn add_package_fixtures(
        &mut self,
        py: Python<'proj>,
        parents: Vec<&'proj dyn HasFixtures<'proj>>,
        current: &'proj dyn HasFixtures<'proj>,
        scope: &[FixtureScope],
        dependencies: Vec<&dyn UsesFixture>,
    ) {
        let called_fixtures = self.add_fixtures_impl(py, parents, current, scope, dependencies);
        self.package_fixtures.extend(called_fixtures);
    }

    pub fn reset_package_fixtures(&mut self) {
        self.package_fixtures.clear();
    }

    pub fn add_module_fixtures(
        &mut self,
        py: Python<'proj>,
        parents: Vec<&'proj dyn HasFixtures<'proj>>,
        current: &'proj dyn HasFixtures<'proj>,
        scope: &[FixtureScope],
        dependencies: Vec<&dyn UsesFixture>,
    ) {
        let called_fixtures = self.add_fixtures_impl(py, parents, current, scope, dependencies);
        self.module_fixtures.extend(called_fixtures);
    }

    pub fn reset_module_fixtures(&mut self) {
        self.module_fixtures.clear();
    }

    pub fn add_function_fixtures(
        &mut self,
        py: Python<'proj>,
        parents: Vec<&'proj dyn HasFixtures<'proj>>,
        current: &'proj dyn HasFixtures<'proj>,
        scope: &[FixtureScope],
        dependencies: Vec<&dyn UsesFixture>,
    ) {
        let called_fixtures = self.add_fixtures_impl(py, parents, current, scope, dependencies);
        self.function_fixtures.extend(called_fixtures);
    }

    pub fn reset_function_fixtures(&mut self) {
        self.function_fixtures.clear();
    }
}

#[cfg(test)]
mod tests {
    use karva_project::{
        project::Project,
        tests::{MockFixture, TestEnv, mock_fixture},
    };

    use super::*;
    use crate::discovery::Discoverer;

    #[test]
    fn test_fixture_manager_add_fixtures_impl_one_dependency() {
        let env = TestEnv::new();
        let fixture = mock_fixture(&[MockFixture {
            name: "x".to_string(),
            scope: "function".to_string(),
            body: "return 1".to_string(),
            args: String::new(),
        }]);
        let tests_dir = env.cwd().join("tests");

        env.create_file("tests/conftest.py", &fixture);
        let test_path = env.create_file("tests/test_1.py", "def test_1(x): pass");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        let tests_package = session.get_package(&tests_dir).unwrap();

        let test_module = tests_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_case("test_1").unwrap();

        let manager = FixtureManager::new();

        Python::with_gil(|py| {
            let called_fixtures = manager.add_fixtures_impl(
                py,
                Vec::new(),
                &tests_package,
                &[FixtureScope::Function],
                vec![first_test_function],
            );
            assert!(called_fixtures.contains_key("x"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_two_dependencies() {
        let env = TestEnv::new();
        let fixture_x = mock_fixture(&[MockFixture {
            name: "x".to_string(),
            scope: "function".to_string(),
            body: "return 2".to_string(),
            args: String::new(),
        }]);
        let fixture_y = mock_fixture(&[MockFixture {
            name: "y".to_string(),
            scope: "function".to_string(),
            body: "return 1".to_string(),
            args: "x".to_string(),
        }]);
        let tests_dir = env.cwd().join("tests");
        let inner_dir = tests_dir.join("inner");

        env.create_file("tests/conftest.py", &fixture_x);
        env.create_file("tests/inner/conftest.py", &fixture_y);
        let test_path = env.create_file("tests/inner/test_1.py", "def test_1(y): pass");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_case("test_1").unwrap();

        let manager = FixtureManager::new();

        Python::with_gil(|py| {
            let called_fixtures = manager.add_fixtures_impl(
                py,
                vec![&tests_package],
                inner_package,
                &[FixtureScope::Function],
                vec![first_test_function],
            );

            assert!(called_fixtures.contains_key("x"));
            assert!(called_fixtures.contains_key("y"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_two_dependencies_in_parent() {
        let env = TestEnv::new();
        let fixture_x = mock_fixture(&[
            MockFixture {
                name: "x".to_string(),
                scope: "function".to_string(),
                body: "return 2".to_string(),
                args: String::new(),
            },
            MockFixture {
                name: "y".to_string(),
                scope: "function".to_string(),
                body: "return 1".to_string(),
                args: "x".to_string(),
            },
        ]);
        let tests_dir = env.cwd().join("tests");
        let inner_dir = tests_dir.join("inner");

        env.create_file("tests/conftest.py", &fixture_x);
        let test_path = env.create_file("tests/inner/test_1.py", "def test_1(y): pass");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        println!("session: {:#?}", session);

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_case("test_1").unwrap();

        let manager = FixtureManager::new();

        Python::with_gil(|py| {
            let called_fixtures = manager.add_fixtures_impl(
                py,
                vec![],
                tests_package,
                &[FixtureScope::Function],
                vec![first_test_function],
            );

            assert!(called_fixtures.contains_key("x"));
            assert!(called_fixtures.contains_key("y"));
        });
    }
}
