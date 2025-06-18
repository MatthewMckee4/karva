use std::collections::HashMap;

use pyo3::{prelude::*, types::PyAny};

use crate::{
    fixture::{Fixture, FixtureScope, HasFixtures, UsesFixture},
    package::Package,
};

#[derive(Debug, Default)]
pub struct FixtureManager<'proj> {
    session: HashMap<String, Bound<'proj, PyAny>>,
    module: HashMap<String, Bound<'proj, PyAny>>,
    package: HashMap<String, Bound<'proj, PyAny>>,
    function: HashMap<String, Bound<'proj, PyAny>>,
}

impl<'proj> FixtureManager<'proj> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            session: HashMap::new(),
            module: HashMap::new(),
            package: HashMap::new(),
            function: HashMap::new(),
        }
    }

    #[must_use]
    pub fn get_fixture(&self, fixture_name: &str) -> Option<Bound<'proj, PyAny>> {
        self.all_fixtures().get(fixture_name).cloned()
    }

    #[must_use]
    pub fn contains_fixture(&self, fixture_name: &str) -> bool {
        self.all_fixtures().contains_key(fixture_name)
    }

    #[must_use]
    pub fn all_fixtures(&self) -> HashMap<String, Bound<'proj, PyAny>> {
        let mut fixtures = HashMap::new();
        fixtures.extend(self.session.iter().map(|(k, v)| (k.clone(), v.clone())));
        fixtures.extend(self.module.iter().map(|(k, v)| (k.clone(), v.clone())));
        fixtures.extend(self.package.iter().map(|(k, v)| (k.clone(), v.clone())));
        fixtures.extend(self.function.iter().map(|(k, v)| (k.clone(), v.clone())));
        fixtures
    }

    pub fn insert_fixture(&mut self, fixture_return: Bound<'proj, PyAny>, fixture: &'proj Fixture) {
        match fixture.scope() {
            FixtureScope::Session => self
                .session
                .insert(fixture.name().to_string(), fixture_return),
            FixtureScope::Module => self
                .module
                .insert(fixture.name().to_string(), fixture_return),
            FixtureScope::Package => self
                .package
                .insert(fixture.name().to_string(), fixture_return),
            FixtureScope::Function => self
                .function
                .insert(fixture.name().to_string(), fixture_return),
        };
    }

    // TODO: This is a bit of a mess.
    // This is sued to recursively resolve all of the dependencies of a fixture.
    fn ensure_fixture_dependencies(
        &mut self,
        py: Python<'proj>,
        parents: &[&'proj Package<'proj>],
        current: &'proj dyn HasFixtures<'proj>,
        fixture: &'proj Fixture,
    ) {
        if self.get_fixture(fixture.name()).is_some() {
            // We have already called this fixture. So we can just return.
            return;
        }

        println!("Ensuring fixture dependencies for {}", fixture.name());

        // To ensure we can call the current fixture, we must first look at all of its dependencies,
        // and resolve them first.
        let current_dependencies = fixture.dependencies();

        println!("{:?}", current_dependencies);

        // We need to get all of the fixtures in the current scope.
        let current_all_fixtures = current.all_fixtures(&[]);

        for dependency in &current_dependencies {
            let mut found = false;
            for fixture in &current_all_fixtures {
                if fixture.name() == dependency {
                    self.ensure_fixture_dependencies(py, parents, current, fixture);
                    found = true;
                    break;
                }
            }

            // We did not find the dependency in the current scope.
            // So we must try the parent scopes.
            if !found {
                let mut parents_above_current_parent = parents.to_vec();
                let mut i = parents.len();
                while i > 0 {
                    i -= 1;
                    let parent = &parents[i];
                    parents_above_current_parent.truncate(i);

                    let parent_fixture = (*parent).get_fixture(dependency);

                    if let Some(parent_fixture) = parent_fixture {
                        self.ensure_fixture_dependencies(
                            py,
                            &parents_above_current_parent,
                            *parent,
                            parent_fixture,
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
    pub fn add_fixtures(
        &mut self,
        py: Python<'proj>,
        parents: &[&'proj Package<'proj>],
        current: &'proj dyn HasFixtures<'proj>,
        scopes: &[FixtureScope],
        dependencies: &[&dyn UsesFixture],
    ) {
        let fixtures = current.fixtures(scopes, dependencies);

        println!("{:?}", fixtures);

        for fixture in &fixtures {
            if scopes.contains(fixture.scope()) {
                self.ensure_fixture_dependencies(py, parents, current, fixture);
            }
        }
    }

    pub fn reset_session_fixtures(&mut self) {
        self.session.clear();
    }

    pub fn reset_package_fixtures(&mut self) {
        self.package.clear();
    }

    pub fn reset_module_fixtures(&mut self) {
        self.module.clear();
    }

    pub fn reset_function_fixtures(&mut self) {
        self.function.clear();
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

        env.create_file(tests_dir.join("conftest.py").as_std_path(), &fixture);
        let test_path = env.create_file(
            tests_dir.join("test_1.py").as_std_path(),
            "def test_1(x): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        println!("{:?}", session.display());

        let tests_package = session.get_package(&tests_dir).unwrap();

        let test_module = tests_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_case("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[],
                &tests_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            println!("{:?}", manager);

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

        env.create_file(tests_dir.join("conftest.py").as_std_path(), &fixture_x);
        env.create_file(inner_dir.join("conftest.py").as_std_path(), &fixture_y);
        let test_path = env.create_file(
            inner_dir.join("test_1.py").as_std_path(),
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
                &[tests_package],
                inner_package,
                &[FixtureScope::Function],
                &[first_test_function],
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

        env.create_file(tests_dir.join("conftest.py").as_std_path(), &fixture_x);
        let test_path = env.create_file(
            inner_dir.join("test_1.py").as_std_path(),
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
                &[],
                tests_package,
                &[FixtureScope::Function],
                &[first_test_function],
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

        env.create_file(tests_dir.join("conftest.py").as_std_path(), &fixture_x);
        env.create_file(inner_dir.join("conftest.py").as_std_path(), &fixture_y);
        env.create_file(
            inner_inner_dir.join("conftest.py").as_std_path(),
            &fixture_z,
        );
        let test_path = env.create_file(
            inner_inner_dir.join("test_1.py").as_std_path(),
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
                &[tests_package, inner_package],
                inner_inner_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.contains_fixture("x"));
            assert!(manager.contains_fixture("y"));
            assert!(manager.contains_fixture("z"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_two_dependencies_different_scopes() {
        let env = TestEnv::new();
        let fixture_x = mock_fixture(&[MockFixture {
            name: "x".to_string(),
            scope: "module".to_string(),
            body: "return 2".to_string(),
            args: String::new(),
        }]);
        let fixture_y_z = mock_fixture(&[
            MockFixture {
                name: "y".to_string(),
                scope: "function".to_string(),
                body: "return 1".to_string(),
                args: "x".to_string(),
            },
            MockFixture {
                name: "z".to_string(),
                scope: "function".to_string(),
                body: "return 1".to_string(),
                args: "x".to_string(),
            },
        ]);
        let tests_dir = env.create_tests_dir();
        let inner_dir = tests_dir.join("inner");

        env.create_file(tests_dir.join("conftest.py").as_std_path(), &fixture_x);
        env.create_file(inner_dir.join("conftest.py").as_std_path(), &fixture_y_z);
        let test_path = env.create_file(
            inner_dir.join("test_1.py").as_std_path(),
            "def test_1(y, z): pass",
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
                &[tests_package],
                inner_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.module.contains_key("x"));
            assert!(manager.function.contains_key("y"));
            assert!(manager.function.contains_key("z"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_three_dependencies_different_scopes() {
        let env = TestEnv::new();
        let fixture_x = mock_fixture(&[MockFixture {
            name: "x".to_string(),
            scope: "session".to_string(),
            body: "return 2".to_string(),
            args: String::new(),
        }]);
        let fixture_y = mock_fixture(&[MockFixture {
            name: "y".to_string(),
            scope: "module".to_string(),
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

        env.create_file(tests_dir.join("conftest.py").as_std_path(), &fixture_x);
        env.create_file(inner_dir.join("conftest.py").as_std_path(), &fixture_y);
        env.create_file(
            inner_inner_dir.join("conftest.py").as_std_path(),
            &fixture_z,
        );
        let test_path = env.create_file(
            inner_inner_dir.join("test_1.py").as_std_path(),
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
                &[tests_package, inner_package],
                inner_inner_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.session.contains_key("x"));
            assert!(manager.module.contains_key("y"));
            assert!(manager.function.contains_key("z"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_three_dependencies_different_scopes_with_fixture_in_function()
     {
        let env = TestEnv::new();

        let fixtures = mock_fixture(&[
            MockFixture {
                name: "x".to_string(),
                scope: "module".to_string(),
                body: "return 1".to_string(),
                args: String::new(),
            },
            MockFixture {
                name: "y".to_string(),
                scope: "function".to_string(),
                body: "return 1".to_string(),
                args: "x".to_string(),
            },
            MockFixture {
                name: "z".to_string(),
                scope: "function".to_string(),
                body: "return 1".to_string(),
                args: "x, y".to_string(),
            },
        ]);
        let tests_dir = env.create_tests_dir();
        let inner_dir = tests_dir.join("inner");

        env.create_file(tests_dir.join("conftest.py").as_std_path(), &fixtures);
        let test_path = env.create_file(
            inner_dir.join("test_1.py").as_std_path(),
            "def test_1(z): pass",
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
                &[],
                tests_package,
                &[FixtureScope::Function, FixtureScope::Module],
                &[first_test_function],
            );

            assert!(manager.module.contains_key("x"));
            assert!(manager.function.contains_key("y"));
            assert!(manager.function.contains_key("z"));
        });
    }

    #[test]
    fn test_fixture_manager_complex_nested_structure_with_session_fixtures() {
        let env = TestEnv::new();

        let root_fixtures = mock_fixture(&[MockFixture {
            name: "database".to_string(),
            scope: "session".to_string(),
            body: "return 'db_connection'".to_string(),
            args: String::new(),
        }]);

        let api_fixtures = mock_fixture(&[MockFixture {
            name: "api_client".to_string(),
            scope: "package".to_string(),
            body: "return 'api_client'".to_string(),
            args: "database".to_string(),
        }]);

        let user_fixtures = mock_fixture(&[MockFixture {
            name: "user".to_string(),
            scope: "module".to_string(),
            body: "return 'test_user'".to_string(),
            args: "api_client".to_string(),
        }]);

        let auth_fixtures = mock_fixture(&[MockFixture {
            name: "auth_token".to_string(),
            scope: "function".to_string(),
            body: "return 'token123'".to_string(),
            args: "user".to_string(),
        }]);

        let tests_dir = env.create_tests_dir();
        let api_dir = tests_dir.join("api");
        let users_dir = api_dir.join("users");

        env.create_file(tests_dir.join("conftest.py").as_std_path(), &root_fixtures);
        env.create_file(api_dir.join("conftest.py").as_std_path(), &api_fixtures);
        env.create_file(users_dir.join("conftest.py").as_std_path(), &user_fixtures);
        let test_path = env.create_file(
            users_dir.join("test_user_auth.py").as_std_path(),
            &format!("{auth_fixtures}\ndef test_user_login(auth_token): pass"),
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        let tests_package = session.get_package(&tests_dir).unwrap();
        let api_package = tests_package.get_package(&api_dir).unwrap();
        let users_package = api_package.get_package(&users_dir).unwrap();
        let test_module = users_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_case("test_user_login").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[tests_package, api_package, users_package],
                test_module,
                &[
                    FixtureScope::Session,
                    FixtureScope::Package,
                    FixtureScope::Module,
                    FixtureScope::Function,
                ],
                &[test_function],
            );

            assert!(manager.package.contains_key("api_client"));
            assert!(manager.module.contains_key("user"));
            assert!(manager.function.contains_key("auth_token"));
            assert!(manager.session.contains_key("database"));
        });
    }

    #[test]
    fn test_fixture_manager_multiple_packages_same_level() {
        let env = TestEnv::new();

        let shared_fixtures = mock_fixture(&[MockFixture {
            name: "config".to_string(),
            scope: "session".to_string(),
            body: "return {'env': 'test'}".to_string(),
            args: String::new(),
        }]);

        let package_a_fixtures = mock_fixture(&[MockFixture {
            name: "service_a".to_string(),
            scope: "package".to_string(),
            body: "return 'service_a'".to_string(),
            args: "config".to_string(),
        }]);

        let package_b_fixtures = mock_fixture(&[MockFixture {
            name: "service_b".to_string(),
            scope: "package".to_string(),
            body: "return 'service_b'".to_string(),
            args: "config".to_string(),
        }]);

        let tests_dir = env.create_tests_dir();
        let package_a_dir = tests_dir.join("package_a");
        let package_b_dir = tests_dir.join("package_b");

        env.create_file(
            tests_dir.join("conftest.py").as_std_path(),
            &shared_fixtures,
        );
        env.create_file(
            package_a_dir.join("conftest.py").as_std_path(),
            &package_a_fixtures,
        );
        env.create_file(
            package_b_dir.join("conftest.py").as_std_path(),
            &package_b_fixtures,
        );

        let test_a_path = env.create_file(
            package_a_dir.join("test_a.py").as_std_path(),
            "def test_a(service_a): pass",
        );
        let test_b_path = env.create_file(
            package_b_dir.join("test_b.py").as_std_path(),
            "def test_b(service_b): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        let tests_package = session.get_package(&tests_dir).unwrap();
        let package_a = tests_package.get_package(&package_a_dir).unwrap();
        let package_b = tests_package.get_package(&package_b_dir).unwrap();

        let module_a = package_a.get_module(&test_a_path).unwrap();
        let module_b = package_b.get_module(&test_b_path).unwrap();

        let test_a = module_a.get_test_case("test_a").unwrap();
        let test_b = module_b.get_test_case("test_b").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[tests_package],
                package_a,
                &[FixtureScope::Session, FixtureScope::Package],
                &[test_a],
            );

            assert!(manager.session.contains_key("config"));
            assert!(manager.package.contains_key("service_a"));

            manager.reset_package_fixtures();

            manager.add_fixtures(
                py,
                &[tests_package],
                package_b,
                &[FixtureScope::Session, FixtureScope::Package],
                &[test_b],
            );

            assert!(manager.session.contains_key("config"));
            assert!(manager.package.contains_key("service_b"));
            assert!(!manager.package.contains_key("service_a"));
        });
    }

    #[test]
    fn test_fixture_manager_fixture_override_in_nested_packages() {
        let env = TestEnv::new();

        let root_fixtures = mock_fixture(&[MockFixture {
            name: "data".to_string(),
            scope: "function".to_string(),
            body: "return 'root_data'".to_string(),
            args: String::new(),
        }]);

        let child_fixtures = mock_fixture(&[MockFixture {
            name: "data".to_string(),
            scope: "function".to_string(),
            body: "return 'child_data'".to_string(),
            args: String::new(),
        }]);

        let tests_dir = env.create_tests_dir();
        let child_dir = tests_dir.join("child");

        env.create_file(tests_dir.join("conftest.py").as_std_path(), &root_fixtures);
        env.create_file(child_dir.join("conftest.py").as_std_path(), &child_fixtures);

        let root_test_path = env.create_file(
            tests_dir.join("test_root.py").as_std_path(),
            "def test_root(data): pass",
        );
        let child_test_path = env.create_file(
            child_dir.join("test_child.py").as_std_path(),
            "def test_child(data): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        let tests_package = session.get_package(&tests_dir).unwrap();
        let child_package = tests_package.get_package(&child_dir).unwrap();

        let root_module = tests_package.get_module(&root_test_path).unwrap();
        let child_module = child_package.get_module(&child_test_path).unwrap();

        let root_test = root_module.get_test_case("test_root").unwrap();
        let child_test = child_module.get_test_case("test_child").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Function],
                &[root_test],
            );

            manager.reset_function_fixtures();
            manager.add_fixtures(
                py,
                &[tests_package],
                child_package,
                &[FixtureScope::Function],
                &[child_test],
            );

            assert!(manager.function.contains_key("data"));
        });
    }

    #[test]
    fn test_fixture_manager_multiple_dependent_fixtures_same_scope() {
        let env = TestEnv::new();

        let fixtures = mock_fixture(&[
            MockFixture {
                name: "base".to_string(),
                scope: "function".to_string(),
                body: "return 'base'".to_string(),
                args: String::new(),
            },
            MockFixture {
                name: "derived_a".to_string(),
                scope: "function".to_string(),
                body: "return f'{base}_a'".to_string(),
                args: "base".to_string(),
            },
            MockFixture {
                name: "derived_b".to_string(),
                scope: "function".to_string(),
                body: "return f'{base}_b'".to_string(),
                args: "base".to_string(),
            },
            MockFixture {
                name: "combined".to_string(),
                scope: "function".to_string(),
                body: "return f'{derived_a}_{derived_b}'".to_string(),
                args: "derived_a, derived_b".to_string(),
            },
        ]);

        let tests_dir = env.create_tests_dir();
        env.create_file(tests_dir.join("conftest.py").as_std_path(), &fixtures);
        let test_path = env.create_file(
            tests_dir.join("test_combined.py").as_std_path(),
            "def test_combined(combined): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        let tests_package = session.get_package(&tests_dir).unwrap();
        let test_module = tests_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_case("test_combined").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Function],
                &[test_function],
            );

            assert!(manager.function.contains_key("base"));
            assert!(manager.function.contains_key("derived_a"));
            assert!(manager.function.contains_key("derived_b"));
            assert!(manager.function.contains_key("combined"));
        });
    }

    #[test]
    fn test_fixture_manager_deep_nesting_five_levels() {
        let env = TestEnv::new();

        let level1_fixtures = mock_fixture(&[MockFixture {
            name: "level1".to_string(),
            scope: "session".to_string(),
            body: "return 'l1'".to_string(),
            args: String::new(),
        }]);

        let level2_fixtures = mock_fixture(&[MockFixture {
            name: "level2".to_string(),
            scope: "package".to_string(),
            body: "return 'l2'".to_string(),
            args: "level1".to_string(),
        }]);

        let level3_fixtures = mock_fixture(&[MockFixture {
            name: "level3".to_string(),
            scope: "module".to_string(),
            body: "return 'l3'".to_string(),
            args: "level2".to_string(),
        }]);

        let level4_fixtures = mock_fixture(&[MockFixture {
            name: "level4".to_string(),
            scope: "function".to_string(),
            body: "return 'l4'".to_string(),
            args: "level3".to_string(),
        }]);

        let level5_fixtures = mock_fixture(&[MockFixture {
            name: "level5".to_string(),
            scope: "function".to_string(),
            body: "return 'l5'".to_string(),
            args: "level4".to_string(),
        }]);

        let tests_dir = env.create_tests_dir();
        let l2_dir = tests_dir.join("level2");
        let l3_dir = l2_dir.join("level3");
        let l4_dir = l3_dir.join("level4");
        let l5_dir = l4_dir.join("level5");

        env.create_file(
            tests_dir.join("conftest.py").as_std_path(),
            &level1_fixtures,
        );
        env.create_file(l2_dir.join("conftest.py").as_std_path(), &level2_fixtures);
        env.create_file(l3_dir.join("conftest.py").as_std_path(), &level3_fixtures);
        env.create_file(l4_dir.join("conftest.py").as_std_path(), &level4_fixtures);
        env.create_file(l5_dir.join("conftest.py").as_std_path(), &level5_fixtures);

        let test_path = env.create_file(
            l5_dir.join("test_deep.py").as_std_path(),
            "def test_deep(level5): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        let l1_package = session.get_package(&tests_dir).unwrap();
        let l2_package = l1_package.get_package(&l2_dir).unwrap();
        let l3_package = l2_package.get_package(&l3_dir).unwrap();
        let l4_package = l3_package.get_package(&l4_dir).unwrap();
        let l5_package = l4_package.get_package(&l5_dir).unwrap();

        let test_module = l5_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_case("test_deep").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[l1_package, l2_package, l3_package, l4_package],
                l5_package,
                &[
                    FixtureScope::Session,
                    FixtureScope::Package,
                    FixtureScope::Module,
                    FixtureScope::Function,
                ],
                &[test_function],
            );

            assert!(manager.session.contains_key("level1"));
            assert!(manager.package.contains_key("level2"));
            assert!(manager.module.contains_key("level3"));
            assert!(manager.function.contains_key("level4"));
            assert!(manager.function.contains_key("level5"));
        });
    }

    #[test]
    fn test_fixture_manager_cross_package_dependencies() {
        let env = TestEnv::new();

        let root_fixtures = mock_fixture(&[MockFixture {
            name: "utils".to_string(),
            scope: "session".to_string(),
            body: "return 'shared_utils'".to_string(),
            args: String::new(),
        }]);

        let package_a_fixtures = mock_fixture(&[MockFixture {
            name: "service_a".to_string(),
            scope: "package".to_string(),
            body: "return f'service_a_{utils}'".to_string(),
            args: "utils".to_string(),
        }]);

        let package_b_fixtures = mock_fixture(&[MockFixture {
            name: "service_b".to_string(),
            scope: "package".to_string(),
            body: "return f'service_b_{utils}'".to_string(),
            args: "utils".to_string(),
        }]);

        let package_c_fixtures = mock_fixture(&[MockFixture {
            name: "integration_service".to_string(),
            scope: "function".to_string(),
            body: "return f'integration_{service_a}_{service_b}'".to_string(),
            args: "service_a, service_b".to_string(),
        }]);

        let tests_dir = env.create_tests_dir();
        let package_a_dir = tests_dir.join("package_a");
        let package_b_dir = tests_dir.join("package_b");
        let package_c_dir = tests_dir.join("package_c");

        env.create_file(tests_dir.join("conftest.py").as_std_path(), &root_fixtures);
        env.create_file(
            package_a_dir.join("conftest.py").as_std_path(),
            &package_a_fixtures,
        );
        env.create_file(
            package_b_dir.join("conftest.py").as_std_path(),
            &package_b_fixtures,
        );
        env.create_file(
            package_c_dir.join("conftest.py").as_std_path(),
            &package_c_fixtures,
        );

        let test_path = env.create_file(
            package_c_dir.join("test_integration.py").as_std_path(),
            "def test_integration(integration_service): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        let tests_package = session.get_package(&tests_dir).unwrap();
        let package_a = tests_package.get_package(&package_a_dir).unwrap();
        let package_b = tests_package.get_package(&package_b_dir).unwrap();
        let package_c = tests_package.get_package(&package_c_dir).unwrap();

        let test_module = package_c.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_case("test_integration").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[tests_package],
                package_a,
                &[FixtureScope::Session, FixtureScope::Package],
                &[],
            );

            manager.add_fixtures(
                py,
                &[tests_package],
                package_b,
                &[FixtureScope::Session, FixtureScope::Package],
                &[],
            );

            manager.add_fixtures(
                py,
                &[tests_package],
                package_c,
                &[
                    FixtureScope::Session,
                    FixtureScope::Package,
                    FixtureScope::Function,
                ],
                &[test_function],
            );

            assert!(manager.session.contains_key("utils"));
            assert!(manager.package.contains_key("service_a"));
            assert!(manager.package.contains_key("service_b"));
            assert!(manager.function.contains_key("integration_service"));
        });
    }

    #[test]
    fn test_fixture_manager_multiple_tests_same_module() {
        let env = TestEnv::new();

        let fixtures = mock_fixture(&[
            MockFixture {
                name: "module_fixture".to_string(),
                scope: "module".to_string(),
                body: "return 'module_data'".to_string(),
                args: String::new(),
            },
            MockFixture {
                name: "function_fixture".to_string(),
                scope: "function".to_string(),
                body: "return 'function_data'".to_string(),
                args: "module_fixture".to_string(),
            },
        ]);

        let tests_dir = env.create_tests_dir();
        env.create_file(tests_dir.join("conftest.py").as_std_path(), &fixtures);

        let test_path = env.create_file(
            tests_dir.join("test_multiple.py").as_std_path(),
            "def test_one(function_fixture): pass\ndef test_two(function_fixture): pass\ndef test_three(module_fixture): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        let tests_package = session.get_package(&tests_dir).unwrap();
        let test_module = tests_package.get_module(&test_path).unwrap();

        let test_one = test_module.get_test_case("test_one").unwrap();
        let test_two = test_module.get_test_case("test_two").unwrap();
        let test_three = test_module.get_test_case("test_three").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Module, FixtureScope::Function],
                &[test_one, test_two, test_three],
            );

            assert!(manager.module.contains_key("module_fixture"));
            assert!(manager.function.contains_key("function_fixture"));
        });
    }

    #[test]
    fn test_fixture_manager_complex_dependency_chain_with_multiple_branches() {
        let env = TestEnv::new();

        let fixtures = mock_fixture(&[
            MockFixture {
                name: "root".to_string(),
                scope: "session".to_string(),
                body: "return 'root'".to_string(),
                args: String::new(),
            },
            MockFixture {
                name: "branch_a1".to_string(),
                scope: "package".to_string(),
                body: "return f'{root}_a1'".to_string(),
                args: "root".to_string(),
            },
            MockFixture {
                name: "branch_a2".to_string(),
                scope: "module".to_string(),
                body: "return f'{branch_a1}_a2'".to_string(),
                args: "branch_a1".to_string(),
            },
            MockFixture {
                name: "branch_b1".to_string(),
                scope: "package".to_string(),
                body: "return f'{root}_b1'".to_string(),
                args: "root".to_string(),
            },
            MockFixture {
                name: "branch_b2".to_string(),
                scope: "module".to_string(),
                body: "return f'{branch_b1}_b2'".to_string(),
                args: "branch_b1".to_string(),
            },
            MockFixture {
                name: "converged".to_string(),
                scope: "function".to_string(),
                body: "return f'{branch_a2}_{branch_b2}'".to_string(),
                args: "branch_a2, branch_b2".to_string(),
            },
        ]);

        let tests_dir = env.create_tests_dir();
        env.create_file(tests_dir.join("conftest.py").as_std_path(), &fixtures);

        let test_path = env.create_file(
            tests_dir.join("test_converged.py").as_std_path(),
            "def test_converged(converged): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        let tests_package = session.get_package(&tests_dir).unwrap();
        let test_module = tests_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_case("test_converged").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[
                    FixtureScope::Session,
                    FixtureScope::Package,
                    FixtureScope::Module,
                    FixtureScope::Function,
                ],
                &[test_function],
            );

            assert!(manager.session.contains_key("root"));
            assert!(manager.package.contains_key("branch_a1"));
            assert!(manager.package.contains_key("branch_b1"));
            assert!(manager.module.contains_key("branch_a2"));
            assert!(manager.module.contains_key("branch_b2"));
            assert!(manager.function.contains_key("converged"));
        });
    }

    #[test]
    fn test_fixture_manager_reset_functions() {
        let env = TestEnv::new();

        let fixtures = mock_fixture(&[
            MockFixture {
                name: "session_fixture".to_string(),
                scope: "session".to_string(),
                body: "return 'session'".to_string(),
                args: String::new(),
            },
            MockFixture {
                name: "package_fixture".to_string(),
                scope: "package".to_string(),
                body: "return 'package'".to_string(),
                args: String::new(),
            },
            MockFixture {
                name: "module_fixture".to_string(),
                scope: "module".to_string(),
                body: "return 'module'".to_string(),
                args: String::new(),
            },
            MockFixture {
                name: "function_fixture".to_string(),
                scope: "function".to_string(),
                body: "return 'function'".to_string(),
                args: String::new(),
            },
        ]);

        let tests_dir = env.create_tests_dir();
        env.create_file(tests_dir.join("conftest.py").as_std_path(), &fixtures);

        let test_path = env.create_file(
            tests_dir.join("test_reset.py").as_std_path(),
            "def test_reset(session_fixture, package_fixture, module_fixture, function_fixture): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Discoverer::new(&project).discover();

        let tests_package = session.get_package(&tests_dir).unwrap();
        let test_module = tests_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_case("test_reset").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[
                    FixtureScope::Session,
                    FixtureScope::Package,
                    FixtureScope::Module,
                    FixtureScope::Function,
                ],
                &[test_function],
            );

            assert!(manager.session.contains_key("session_fixture"));
            assert!(manager.package.contains_key("package_fixture"));
            assert!(manager.module.contains_key("module_fixture"));
            assert!(manager.function.contains_key("function_fixture"));

            manager.reset_function_fixtures();
            assert!(!manager.function.contains_key("function_fixture"));
            assert!(manager.module.contains_key("module_fixture"));

            manager.reset_module_fixtures();
            assert!(!manager.module.contains_key("module_fixture"));
            assert!(manager.package.contains_key("package_fixture"));

            manager.reset_package_fixtures();
            assert!(!manager.package.contains_key("package_fixture"));
            assert!(manager.session.contains_key("session_fixture"));

            manager.reset_session_fixtures();
            assert!(!manager.session.contains_key("session_fixture"));
        });
    }
}
