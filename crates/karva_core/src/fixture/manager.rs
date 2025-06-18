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

    pub fn contains_fixture(&self, fixture_name: &str) -> bool {
        self.all_fixtures().contains_key(fixture_name)
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

    pub fn insert_fixture(&mut self, fixture_return: Bound<'proj, PyAny>, fixture: &'proj Fixture) {
        match fixture.scope() {
            FixtureScope::Session => self
                .session_fixtures
                .insert(fixture.name().to_string(), fixture_return),
            FixtureScope::Module => self
                .module_fixtures
                .insert(fixture.name().to_string(), fixture_return),
            FixtureScope::Package => self
                .package_fixtures
                .insert(fixture.name().to_string(), fixture_return),
            FixtureScope::Function => self
                .function_fixtures
                .insert(fixture.name().to_string(), fixture_return),
        };
    }

    // TODO: This is a bit of a mess.
    // This is sued to recursively resolve all of the dependencies of a fixture.
    fn ensure_fixture_dependencies(
        &mut self,
        py: Python<'proj>,
        parents: Vec<&'proj dyn HasFixtures<'proj>>,
        current: &'proj dyn HasFixtures<'proj>,
        scopes: &[FixtureScope],
        fixture: &'proj Fixture,
    ) {
        if let Some(fixture_return) = self.get_fixture(fixture.name()) {
            // We have already called this fixture. So we can just return.
            self.insert_fixture(fixture_return, fixture);
            return;
        }

        // To ensure we can call the current fixture, we must first look at all of its dependencies,
        // and resolve them first.
        let current_dependencies = fixture.dependencies();

        // We need to get all of the fixtures in the current scope.
        let current_all_fixtures = current.all_fixtures(Vec::new());

        for dependency in &current_dependencies {
            let mut found = false;
            for fixture in &current_all_fixtures {
                if fixture.name() == dependency {
                    self.ensure_fixture_dependencies(py, parents.clone(), current, scopes, fixture);
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
                        );
                    } else {
                        tracing::error!(
                            "Failed to find fixture {} in parent {:?}",
                            dependency,
                            parent
                        );
                    }
                    if self.contains_fixture(dependency) {
                        break;
                    }
                }
            }
        }

        let mut required_fixtures = Vec::new();

        for name in current_dependencies {
            if let Some(fixture) = self.get_fixture(&name) {
                required_fixtures.push(fixture.clone());
            } else {
                tracing::error!("Failed to find fixture {}", name);
                return;
            }
        }

        // I think we can be sure that required_fixtures

        match fixture.call(py, required_fixtures) {
            Ok(fixture_return) => {
                self.insert_fixture(fixture_return, fixture);
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
        &mut self,
        py: Python<'proj>,
        parents: Vec<&'proj dyn HasFixtures<'proj>>,
        current: &'proj dyn HasFixtures<'proj>,
        scopes: &[FixtureScope],
        dependencies: Vec<&dyn UsesFixture>,
    ) {
        let fixtures = current.fixtures(scopes, dependencies.clone());

        for fixture in &fixtures {
            self.ensure_fixture_dependencies(py, parents.clone(), current, scopes, fixture);
        }
    }

    pub fn add_fixtures(
        &mut self,
        py: Python<'proj>,
        parents: Vec<&'proj dyn HasFixtures<'proj>>,
        current: &'proj dyn HasFixtures<'proj>,
        scope: &[FixtureScope],
        dependencies: Vec<&dyn UsesFixture>,
    ) {
        self.add_fixtures_impl(py, parents, current, scope, dependencies);
    }

    pub fn add_session_fixtures(
        &mut self,
        py: Python<'proj>,
        parents: Vec<&'proj dyn HasFixtures<'proj>>,
        current: &'proj dyn HasFixtures<'proj>,
        scope: &[FixtureScope],
        dependencies: Vec<&dyn UsesFixture>,
    ) {
        self.add_fixtures_impl(py, parents, current, scope, dependencies);
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
        self.add_fixtures_impl(py, parents, current, scope, dependencies);
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
        self.add_fixtures_impl(py, parents, current, scope, dependencies);
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
        self.add_fixtures_impl(py, parents, current, scope, dependencies);
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
        let tests_dir = env.create_tests_dir();

        env.create_file(&tests_dir.join("conftest.py").to_string(), &fixture);
        let test_path = env.create_file(
            &tests_dir.join("test_1.py").to_string(),
            "def test_1(x): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        let tests_package = session.get_package(&tests_dir).unwrap();

        let test_module = tests_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_case("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                Vec::new(),
                &tests_package,
                &[FixtureScope::Function],
                vec![first_test_function],
            );
            assert!(manager.contains_fixture("x"));
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
        let tests_dir = env.create_tests_dir();
        let inner_dir = tests_dir.join("inner");

        env.create_file(&tests_dir.join("conftest.py").to_string(), &fixture_x);
        env.create_file(&tests_dir.join("inner/conftest.py").to_string(), &fixture_y);
        let test_path = env.create_file(
            &tests_dir.join("inner/test_1.py").to_string(),
            "def test_1(y): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_case("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                vec![&tests_package],
                inner_package,
                &[FixtureScope::Function],
                vec![first_test_function],
            );

            assert!(manager.contains_fixture("x"));
            assert!(manager.contains_fixture("y"));
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
        let tests_dir = env.create_tests_dir();
        let inner_dir = tests_dir.join("inner");

        env.create_file(&tests_dir.join("conftest.py").to_string(), &fixture_x);
        let test_path = env.create_file(
            &tests_dir.join("inner/test_1.py").to_string(),
            "def test_1(y): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_case("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                vec![],
                tests_package,
                &[FixtureScope::Function],
                vec![first_test_function],
            );

            assert!(manager.contains_fixture("x"));
            assert!(manager.contains_fixture("y"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_three_dependencies() {
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
        let fixture_z = mock_fixture(&[MockFixture {
            name: "z".to_string(),
            scope: "function".to_string(),
            body: "return 3".to_string(),
            args: "y".to_string(),
        }]);
        let tests_dir = env.create_tests_dir();
        let inner_dir = tests_dir.join("inner");
        let inner_inner_dir = inner_dir.join("inner");

        env.create_file(&tests_dir.join("conftest.py").to_string(), &fixture_x);
        env.create_file(&tests_dir.join("inner/conftest.py").to_string(), &fixture_y);
        env.create_file(
            &tests_dir.join("inner/inner/conftest.py").to_string(),
            &fixture_z,
        );
        let test_path = env.create_file(
            &tests_dir.join("inner/inner/test_1.py").to_string(),
            "def test_1(z): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let inner_inner_package = inner_package.get_package(&inner_inner_dir).unwrap();

        let test_module = inner_inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_case("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                vec![&tests_package, &inner_package],
                inner_inner_package,
                &[FixtureScope::Function],
                vec![first_test_function],
            );

            assert!(manager.contains_fixture("x"));
            assert!(manager.contains_fixture("y"));
            assert!(manager.contains_fixture("z"));
        });
    }
}
