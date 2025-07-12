use std::collections::HashMap;

use pyo3::{prelude::*, types::PyAny};

use crate::{
    fixture::{Finalizer, Finalizers, Fixture, FixtureScope, HasFixtures, RequiresFixtures},
    models::DiscoveredPackage,
    utils::partition_iter,
};

#[derive(Debug, Default)]
pub struct FixtureCollection {
    fixtures: HashMap<String, Py<PyAny>>,
    finalizers: Vec<Finalizer>,
}

impl FixtureCollection {
    pub fn insert_fixture(&mut self, fixture_name: String, fixture_return: Py<PyAny>) {
        self.fixtures.insert(fixture_name, fixture_return);
    }

    pub fn insert_finalizer(&mut self, finalizer: Finalizer) {
        self.finalizers.push(finalizer);
    }

    pub fn iter_fixtures(&self) -> impl Iterator<Item = (&String, &Py<PyAny>)> {
        self.fixtures.iter()
    }

    pub fn reset(&mut self) -> Finalizers {
        self.fixtures.clear();
        Finalizers::new(self.finalizers.drain(..).collect())
    }

    #[cfg(test)]
    pub fn contains_fixture(&self, fixture_name: &str) -> bool {
        self.fixtures.contains_key(fixture_name)
    }
}

#[derive(Debug, Default)]
pub struct FixtureManager {
    session: FixtureCollection,
    module: FixtureCollection,
    package: FixtureCollection,
    function: FixtureCollection,
}

impl FixtureManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            session: FixtureCollection::default(),
            module: FixtureCollection::default(),
            package: FixtureCollection::default(),
            function: FixtureCollection::default(),
        }
    }

    #[must_use]
    pub fn get_fixture(&self, fixture_name: &str) -> Option<Py<PyAny>> {
        self.all_fixtures().get(fixture_name).cloned()
    }

    #[must_use]
    pub fn contains_fixture(&self, fixture_name: &str) -> bool {
        self.all_fixtures().contains_key(fixture_name)
    }

    #[must_use]
    pub fn all_fixtures(&self) -> HashMap<String, Py<PyAny>> {
        let mut fixtures = HashMap::new();
        for self_fixtures in [&self.session, &self.module, &self.package, &self.function] {
            fixtures.extend(
                self_fixtures
                    .iter_fixtures()
                    .map(|(k, v)| (k.clone(), v.clone())),
            );
        }
        fixtures
    }

    pub fn insert_fixture(&mut self, fixture_return: Py<PyAny>, fixture: &Fixture) {
        match fixture.scope() {
            FixtureScope::Session => self
                .session
                .insert_fixture(fixture.name().to_string(), fixture_return),
            FixtureScope::Module => self
                .module
                .insert_fixture(fixture.name().to_string(), fixture_return),
            FixtureScope::Package => self
                .package
                .insert_fixture(fixture.name().to_string(), fixture_return),
            FixtureScope::Function => self
                .function
                .insert_fixture(fixture.name().to_string(), fixture_return),
        }
    }

    pub fn insert_finalizer(&mut self, finalizer: Finalizer, scope: &FixtureScope) {
        match scope {
            FixtureScope::Session => self.session.insert_finalizer(finalizer),
            FixtureScope::Module => self.module.insert_finalizer(finalizer),
            FixtureScope::Package => self.package.insert_finalizer(finalizer),
            FixtureScope::Function => self.function.insert_finalizer(finalizer),
        }
    }

    // TODO: This is a bit of a mess.
    // This used to ensure that all of the given dependencies (fixtures) have been called.
    // This first starts with finding all dependencies of the given fixtures, and resolving and calling them first.
    //
    // We take the parents to ensure that if the dependent fixtures are not in the current scope,
    // we can still look for them in the parents.
    fn ensure_fixture_dependencies<'proj>(
        &mut self,
        py: Python<'_>,
        parents: &[&'proj DiscoveredPackage<'proj>],
        current: &'proj dyn HasFixtures<'proj>,
        fixture: &Fixture,
    ) {
        if self.get_fixture(fixture.name()).is_some() {
            // We have already called this fixture. So we can just return.
            return;
        }

        // To ensure we can call the current fixture, we must first look at all of its dependencies,
        // and resolve them first.
        let current_dependencies = fixture.required_fixtures();

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
                for (parent, parents_above_current_parent) in partition_iter(parents) {
                    let parent_fixture = (*parent).get_fixture(dependency);

                    if let Some(parent_fixture) = parent_fixture {
                        self.ensure_fixture_dependencies(
                            py,
                            &parents_above_current_parent,
                            parent,
                            parent_fixture,
                        );
                    }
                    if self.contains_fixture(dependency) {
                        break;
                    }
                }
            }
        }

        match fixture.call(py, self) {
            Ok(fixture_return) => {
                self.insert_fixture(fixture_return.unbind(), fixture);
            }
            Err(e) => {
                tracing::debug!("Failed to call fixture {}: {}", fixture.name(), e);
            }
        }
    }

    pub fn add_fixtures<'proj>(
        &mut self,
        py: Python<'_>,
        parents: &[&'proj DiscoveredPackage<'proj>],
        current: &'proj dyn HasFixtures<'proj>,
        scopes: &[FixtureScope],
        dependencies: &[&dyn RequiresFixtures],
    ) {
        let fixtures = current.fixtures(scopes, dependencies);

        for fixture in fixtures {
            self.ensure_fixture_dependencies(py, parents, current, fixture);
        }
    }

    pub fn reset_session_fixtures(&mut self) -> Finalizers {
        self.session.reset()
    }

    pub fn reset_package_fixtures(&mut self) -> Finalizers {
        self.package.reset()
    }

    pub fn reset_module_fixtures(&mut self) -> Finalizers {
        self.module.reset()
    }

    pub fn reset_function_fixtures(&mut self) -> Finalizers {
        self.function.reset()
    }
}

#[cfg(test)]
mod tests {
    use karva_project::{project::Project, tests::TestEnv};

    use super::*;
    use crate::discovery::Discoverer;

    #[test]
    fn test_fixture_manager_add_fixtures_impl_one_dependency() {
        let env = TestEnv::new();
        let tests_dir = env.create_test_dir();

        env.create_file(
            tests_dir.join("conftest.py"),
            r"
import karva
@karva.fixture(scope='function')
def x():
    return 1
",
        );
        let test_path = env.create_file(tests_dir.join("test_1.py"), "def test_1(x): pass");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let test_module = tests_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[],
                &tests_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.contains_fixture("x"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_two_dependencies() {
        let env = TestEnv::new();
        let fixture_x = r"
import karva
@karva.fixture(scope='function')
def x():
    return 2
";
        let fixture_y = r"
import karva
@karva.fixture(scope='function')
def y(x):
    return 1
";
        let tests_dir = env.create_test_dir();
        let inner_dir = tests_dir.join("inner");

        env.create_file(tests_dir.join("conftest.py"), fixture_x);
        env.create_file(inner_dir.join("conftest.py"), fixture_y);
        let test_path = env.create_file(inner_dir.join("test_1.py"), "def test_1(y): pass");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

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
        let fixture_x = r"
import karva
@karva.fixture(scope='function')
def x():
    return 2
@karva.fixture(scope='function')
def y(x):
    return 1
";

        let tests_dir = env.create_test_dir();
        let inner_dir = tests_dir.join("inner");

        env.create_file(tests_dir.join("conftest.py"), fixture_x);
        let test_path = env.create_file(inner_dir.join("test_1.py"), "def test_1(y): pass");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

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
        let fixture_x = r"
import karva
@karva.fixture(scope='function')
def x():
    return 2
";
        let fixture_y = r"
import karva
@karva.fixture(scope='function')
def y(x):
    return 1
";
        let fixture_z = r"
import karva
@karva.fixture(scope='function')
def z(y):
    return 3
";
        let tests_dir = env.create_test_dir();
        let inner_dir = tests_dir.join("inner");
        let inner_inner_dir = inner_dir.join("inner");

        env.create_file(tests_dir.join("conftest.py"), fixture_x);
        env.create_file(inner_dir.join("conftest.py"), fixture_y);
        env.create_file(inner_inner_dir.join("conftest.py"), fixture_z);
        let test_path = env.create_file(inner_inner_dir.join("test_1.py"), "def test_1(z): pass");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let inner_inner_package = inner_package.get_package(&inner_inner_dir).unwrap();

        let test_module = inner_inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

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
        let fixture_x = r"
import karva
@karva.fixture(scope='module')
def x():
    return 2
";
        let fixture_y_z = r"
import karva
@karva.fixture(scope='function')
def y(x):
    return 1
@karva.fixture(scope='function')
def z(x):
    return 1
";
        let tests_dir = env.create_test_dir();
        let inner_dir = tests_dir.join("inner");

        env.create_file(tests_dir.join("conftest.py"), fixture_x);
        env.create_file(inner_dir.join("conftest.py"), fixture_y_z);
        let test_path = env.create_file(inner_dir.join("test_1.py"), "def test_1(y, z): pass");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[tests_package],
                inner_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.module.contains_fixture("x"));
            assert!(manager.function.contains_fixture("y"));
            assert!(manager.function.contains_fixture("z"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_three_dependencies_different_scopes() {
        let env = TestEnv::new();
        let fixture_x = r"
import karva
@karva.fixture(scope='session')
def x():
    return 2
";
        let fixture_y = r"
import karva
@karva.fixture(scope='module')
def y(x):
    return 1
";
        let fixture_z = r"
import karva
@karva.fixture(scope='function')
def z(y):
    return 3
";
        let tests_dir = env.create_test_dir();
        let inner_dir = tests_dir.join("inner");
        let inner_inner_dir = inner_dir.join("inner");

        env.create_file(tests_dir.join("conftest.py"), fixture_x);
        env.create_file(inner_dir.join("conftest.py"), fixture_y);
        env.create_file(inner_inner_dir.join("conftest.py"), fixture_z);
        let test_path = env.create_file(inner_inner_dir.join("test_1.py"), "def test_1(z): pass");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let inner_inner_package = inner_package.get_package(&inner_inner_dir).unwrap();

        let test_module = inner_inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[tests_package, inner_package],
                inner_inner_package,
                &[FixtureScope::Function],
                &[first_test_function],
            );

            assert!(manager.session.contains_fixture("x"));
            assert!(manager.module.contains_fixture("y"));
            assert!(manager.function.contains_fixture("z"));
        });
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_three_dependencies_different_scopes_with_fixture_in_function()
     {
        let env = TestEnv::new();

        let fixtures = r"
import karva
@karva.fixture(scope='module')
def x():
    return 1
@karva.fixture(scope='function')
def y(x):
    return 1

@karva.fixture(scope='function')
def z(x, y):
    return 1
";

        let tests_dir = env.create_test_dir();
        let inner_dir = tests_dir.join("inner");

        env.create_file(tests_dir.join("conftest.py"), fixtures);
        let test_path = env.create_file(inner_dir.join("test_1.py"), "def test_1(z): pass");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();

        let inner_package = tests_package.get_package(&inner_dir).unwrap();

        let test_module = inner_package.get_module(&test_path).unwrap();

        let first_test_function = test_module.get_test_function("test_1").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Function, FixtureScope::Module],
                &[first_test_function],
            );

            assert!(manager.module.contains_fixture("x"));
            assert!(manager.function.contains_fixture("y"));
            assert!(manager.function.contains_fixture("z"));
        });
    }

    #[test]
    fn test_fixture_manager_complex_nested_structure_with_session_fixtures() {
        let env = TestEnv::new();

        let root_fixtures = r"
import karva
@karva.fixture(scope='session')
def database():
    return 'db_connection'
";

        let api_fixtures = r"
import karva
@karva.fixture(scope='package')
def api_client(database):
    return 'api_client'
";

        let user_fixtures = r"
import karva
@karva.fixture(scope='module')
def user(api_client):
    return 'test_user'
";

        let auth_fixtures = r"
import karva
@karva.fixture(scope='function')
def auth_token(user):
    return 'token123'
";

        let tests_dir = env.create_test_dir();
        let api_dir = tests_dir.join("api");
        let users_dir = api_dir.join("users");

        env.create_file(tests_dir.join("conftest.py"), root_fixtures);
        env.create_file(api_dir.join("conftest.py"), api_fixtures);
        env.create_file(users_dir.join("conftest.py"), user_fixtures);
        let test_path = env.create_file(
            users_dir.join("test_user_auth.py"),
            &format!("{auth_fixtures}\ndef test_user_login(auth_token): pass"),
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let api_package = tests_package.get_package(&api_dir).unwrap();
        let users_package = api_package.get_package(&users_dir).unwrap();
        let test_module = users_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_function("test_user_login").unwrap();

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

            assert!(manager.package.contains_fixture("api_client"));
            assert!(manager.module.contains_fixture("user"));
            assert!(manager.function.contains_fixture("auth_token"));
            assert!(manager.session.contains_fixture("database"));
        });
    }

    #[test]
    fn test_fixture_manager_multiple_packages_same_level() {
        let env = TestEnv::new();

        let shared_fixtures = r"
import karva
@karva.fixture(scope='session')
def config():
    return {'env': 'test'}
";

        let package_a_fixtures = r"
import karva
@karva.fixture(scope='package')
def service_a(config):
    return 'service_a'
";

        let package_b_fixtures = r"
import karva
@karva.fixture(scope='package')
def service_b(config):
    return 'service_b'
";

        let tests_dir = env.create_test_dir();
        let package_a_dir = tests_dir.join("package_a");
        let package_b_dir = tests_dir.join("package_b");

        env.create_file(tests_dir.join("conftest.py"), shared_fixtures);
        env.create_file(package_a_dir.join("conftest.py"), package_a_fixtures);
        env.create_file(package_b_dir.join("conftest.py"), package_b_fixtures);

        let test_a_path = env.create_file(
            package_a_dir.join("test_a.py"),
            "def test_a(service_a): pass",
        );
        let test_b_path = env.create_file(
            package_b_dir.join("test_b.py"),
            "def test_b(service_b): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let package_a = tests_package.get_package(&package_a_dir).unwrap();
        let package_b = tests_package.get_package(&package_b_dir).unwrap();

        let module_a = package_a.get_module(&test_a_path).unwrap();
        let module_b = package_b.get_module(&test_b_path).unwrap();

        let test_a = module_a.get_test_function("test_a").unwrap();
        let test_b = module_b.get_test_function("test_b").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[tests_package],
                package_a,
                &[FixtureScope::Session, FixtureScope::Package],
                &[test_a],
            );

            assert!(manager.session.contains_fixture("config"));
            assert!(manager.package.contains_fixture("service_a"));

            manager.reset_package_fixtures();

            manager.add_fixtures(
                py,
                &[tests_package],
                package_b,
                &[FixtureScope::Session, FixtureScope::Package],
                &[test_b],
            );

            assert!(manager.session.contains_fixture("config"));
            assert!(manager.package.contains_fixture("service_b"));
            assert!(!manager.package.contains_fixture("service_a"));
        });
    }

    #[test]
    fn test_fixture_manager_fixture_override_in_nested_packages() {
        let env = TestEnv::new();

        let root_fixtures = r"
import karva
@karva.fixture(scope='function')
def data():
    return 'root_data'
";

        let child_fixtures = r"
import karva
@karva.fixture(scope='function')
def data():
    return 'child_data'
";

        let tests_dir = env.create_test_dir();
        let child_dir = tests_dir.join("child");

        env.create_file(tests_dir.join("conftest.py"), root_fixtures);
        env.create_file(child_dir.join("conftest.py"), child_fixtures);

        let root_test_path =
            env.create_file(tests_dir.join("test_root.py"), "def test_root(data): pass");
        let child_test_path = env.create_file(
            child_dir.join("test_child.py"),
            "def test_child(data): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let child_package = tests_package.get_package(&child_dir).unwrap();

        let root_module = tests_package.get_module(&root_test_path).unwrap();
        let child_module = child_package.get_module(&child_test_path).unwrap();

        let root_test = root_module.get_test_function("test_root").unwrap();
        let child_test = child_module.get_test_function("test_child").unwrap();

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

            assert!(manager.function.contains_fixture("data"));
        });
    }

    #[test]
    fn test_fixture_manager_multiple_dependent_fixtures_same_scope() {
        let env = TestEnv::new();

        let fixtures = r"
import karva
@karva.fixture(scope='function')
def base():
    return 'base'
@karva.fixture(scope='function')
def derived_a(base):
    return f'{base}_a'
@karva.fixture(scope='function')
def derived_b(base):
    return f'{base}_b'
@karva.fixture(scope='function')
def combined(derived_a, derived_b):
    return f'{derived_a}_{derived_b}'
";

        let tests_dir = env.create_test_dir();
        env.create_file(tests_dir.join("conftest.py"), fixtures);
        let test_path = env.create_file(
            tests_dir.join("test_combined.py"),
            "def test_combined(combined): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let test_module = tests_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_function("test_combined").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Function],
                &[test_function],
            );

            assert!(manager.function.contains_fixture("base"));
            assert!(manager.function.contains_fixture("derived_a"));
            assert!(manager.function.contains_fixture("derived_b"));
            assert!(manager.function.contains_fixture("combined"));
        });
    }

    #[test]
    fn test_fixture_manager_deep_nesting_five_levels() {
        let env = TestEnv::new();

        let level1_fixtures = r"
import karva
@karva.fixture(scope='session')
def level1():
    return 'l1'
";
        let level2_fixtures = r"
import karva
@karva.fixture(scope='package')
def level2(level1):
    return 'l2'
";

        let level3_fixtures = r"
import karva
@karva.fixture(scope='module')
def level3(level2):
    return 'l3'
";

        let level4_fixtures = r"
import karva
@karva.fixture(scope='function')
def level4(level3):
    return 'l4'
";

        let level5_fixtures = r"
import karva
@karva.fixture(scope='function')
def level5(level4):
    return 'l5'
";

        let tests_dir = env.create_test_dir();
        let l2_dir = tests_dir.join("level2");
        let l3_dir = l2_dir.join("level3");
        let l4_dir = l3_dir.join("level4");
        let l5_dir = l4_dir.join("level5");

        env.create_file(tests_dir.join("conftest.py"), level1_fixtures);
        env.create_file(l2_dir.join("conftest.py"), level2_fixtures);
        env.create_file(l3_dir.join("conftest.py"), level3_fixtures);
        env.create_file(l4_dir.join("conftest.py"), level4_fixtures);
        env.create_file(l5_dir.join("conftest.py"), level5_fixtures);

        let test_path = env.create_file(l5_dir.join("test_deep.py"), "def test_deep(level5): pass");

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let l1_package = session.get_package(&tests_dir).unwrap();
        let l2_package = l1_package.get_package(&l2_dir).unwrap();
        let l3_package = l2_package.get_package(&l3_dir).unwrap();
        let l4_package = l3_package.get_package(&l4_dir).unwrap();
        let l5_package = l4_package.get_package(&l5_dir).unwrap();

        let test_module = l5_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_function("test_deep").unwrap();

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

            assert!(manager.session.contains_fixture("level1"));
            assert!(manager.package.contains_fixture("level2"));
            assert!(manager.module.contains_fixture("level3"));
            assert!(manager.function.contains_fixture("level4"));
            assert!(manager.function.contains_fixture("level5"));
        });
    }

    #[test]
    fn test_fixture_manager_cross_package_dependencies() {
        let env = TestEnv::new();

        let root_fixtures = r"
import karva
@karva.fixture(scope='session')
def utils():
    return 'shared_utils'
";
        let package_a_fixtures = r"
import karva
@karva.fixture(scope='package')
def service_a(utils):
    return f'service_a_{utils}'
";

        let package_b_fixtures = r"
import karva
@karva.fixture(scope='package')
def service_b(utils):
    return f'service_b_{utils}'
";

        let package_c_fixtures = r"
import karva
@karva.fixture(scope='function')
def integration_service(service_a, service_b):
    return f'integration_{service_a}_{service_b}'
";

        let tests_dir = env.create_test_dir();
        let package_a_dir = tests_dir.join("package_a");
        let package_b_dir = tests_dir.join("package_b");
        let package_c_dir = tests_dir.join("package_c");

        env.create_file(tests_dir.join("conftest.py"), root_fixtures);
        env.create_file(package_a_dir.join("conftest.py"), package_a_fixtures);
        env.create_file(package_b_dir.join("conftest.py"), package_b_fixtures);
        env.create_file(package_c_dir.join("conftest.py"), package_c_fixtures);

        let test_path = env.create_file(
            package_c_dir.join("test_integration.py"),
            "def test_integration(integration_service): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let package_a = tests_package.get_package(&package_a_dir).unwrap();
        let package_b = tests_package.get_package(&package_b_dir).unwrap();
        let package_c = tests_package.get_package(&package_c_dir).unwrap();

        let test_module = package_c.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_function("test_integration").unwrap();

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

            assert!(manager.session.contains_fixture("utils"));
            assert!(manager.package.contains_fixture("service_a"));
            assert!(manager.package.contains_fixture("service_b"));
            assert!(manager.function.contains_fixture("integration_service"));
        });
    }

    #[test]
    fn test_fixture_manager_multiple_tests_same_module() {
        let env = TestEnv::new();

        let fixtures = r"
import karva
@karva.fixture(scope='module')
def module_fixture():
    return 'module_data'
import karva
@karva.fixture(scope='function')
def function_fixture(module_fixture):
    return 'function_data'
";

        let tests_dir = env.create_test_dir();
        env.create_file(tests_dir.join("conftest.py"), fixtures);

        let test_path = env.create_file(
            tests_dir.join("test_multiple.py"),
            "def test_one(function_fixture): pass\ndef test_two(function_fixture): pass\ndef test_three(module_fixture): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let test_module = tests_package.get_module(&test_path).unwrap();

        let test_one = test_module.get_test_function("test_one").unwrap();
        let test_two = test_module.get_test_function("test_two").unwrap();
        let test_three = test_module.get_test_function("test_three").unwrap();

        Python::with_gil(|py| {
            let mut manager = FixtureManager::new();

            manager.add_fixtures(
                py,
                &[],
                tests_package,
                &[FixtureScope::Module, FixtureScope::Function],
                &[test_one, test_two, test_three],
            );

            assert!(manager.module.contains_fixture("module_fixture"));
            assert!(manager.function.contains_fixture("function_fixture"));
        });
    }

    #[test]
    fn test_fixture_manager_complex_dependency_chain_with_multiple_branches() {
        let env = TestEnv::new();

        let fixtures = r"
import karva
@karva.fixture(scope='session')
def root():
    return 'root'
@karva.fixture(scope='package')
def branch_a1(root):
    return f'{root}_a1'
@karva.fixture(scope='module')
def branch_a2(branch_a1):
    return f'{branch_a1}_a2'
@karva.fixture(scope='package')
def branch_b1(root):
    return f'{root}_b1'
@karva.fixture(scope='module')
def branch_b2(branch_b1):
    return f'{branch_b1}_b2'
@karva.fixture(scope='function')
def converged(branch_a2, branch_b2):
    return f'{branch_a2}_{branch_b2}'

";

        let tests_dir = env.create_test_dir();
        env.create_file(tests_dir.join("conftest.py"), fixtures);

        let test_path = env.create_file(
            tests_dir.join("test_converged.py"),
            "def test_converged(converged): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let test_module = tests_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_function("test_converged").unwrap();

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

            assert!(manager.session.contains_fixture("root"));
            assert!(manager.package.contains_fixture("branch_a1"));
            assert!(manager.package.contains_fixture("branch_b1"));
            assert!(manager.module.contains_fixture("branch_a2"));
            assert!(manager.module.contains_fixture("branch_b2"));
            assert!(manager.function.contains_fixture("converged"));
        });
    }

    #[test]
    fn test_fixture_manager_reset_functions() {
        let env = TestEnv::new();

        let fixtures = r"
import karva
@karva.fixture(scope='session')
def session_fixture():
    return 'session'
@karva.fixture(scope='package')
def package_fixture():
    return 'package'
@karva.fixture(scope='module')
def module_fixture():
    return 'module'
@karva.fixture(scope='function')
def function_fixture():
    return 'function'
";

        let tests_dir = env.create_test_dir();
        env.create_file(tests_dir.join("conftest.py"), fixtures);

        let test_path = env.create_file(
            tests_dir.join("test_reset.py"),
            "def test_reset(session_fixture, package_fixture, module_fixture, function_fixture): pass",
        );

        let project = Project::new(env.cwd(), vec![env.cwd()]);
        let (session, _) = Python::with_gil(|py| Discoverer::new(&project).discover(py));

        let tests_package = session.get_package(&tests_dir).unwrap();
        let test_module = tests_package.get_module(&test_path).unwrap();
        let test_function = test_module.get_test_function("test_reset").unwrap();

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

            assert!(manager.session.contains_fixture("session_fixture"));
            assert!(manager.package.contains_fixture("package_fixture"));
            assert!(manager.module.contains_fixture("module_fixture"));
            assert!(manager.function.contains_fixture("function_fixture"));

            manager.reset_function_fixtures();
            assert!(!manager.function.contains_fixture("function_fixture"));
            assert!(manager.module.contains_fixture("module_fixture"));

            manager.reset_module_fixtures();
            assert!(!manager.module.contains_fixture("module_fixture"));
            assert!(manager.package.contains_fixture("package_fixture"));

            manager.reset_package_fixtures();
            assert!(!manager.package.contains_fixture("package_fixture"));
            assert!(manager.session.contains_fixture("session_fixture"));

            manager.reset_session_fixtures();
            assert!(!manager.session.contains_fixture("session_fixture"));
        });
    }
}
